use anyhow::Result;
use num::BigUint;
use plonky2::field::extension::{Extendable, FieldExtension};
use plonky2::field::packed::PackedField;
use plonky2::hash::hash_types::RichField;
use plonky2::plonk::circuit_builder::CircuitBuilder;

use super::*;
use crate::arithmetic::builder::ChipBuilder;
use crate::arithmetic::chip::ChipParameters;
use crate::arithmetic::instruction::Instruction;
use crate::arithmetic::polynomial::{Polynomial, PolynomialGadget, PolynomialOps};
use crate::arithmetic::register::{Array, MemorySlice, RegisterSerializable, U16Register};
use crate::arithmetic::trace::TraceHandle;
use crate::arithmetic::utils::{extract_witness_and_shift, split_digits, to_field_iter};
use crate::vars::{StarkEvaluationTargets, StarkEvaluationVars};

#[derive(Debug, Clone, Copy)]
pub struct FpMulConst<P: FieldParameters> {
    a: FieldRegister<P>,
    c: [u16; MAX_NB_LIMBS],
    result: FieldRegister<P>,
    carry: FieldRegister<P>,
    witness_low: Array<U16Register>,
    witness_high: Array<U16Register>,
}

impl<L: ChipParameters<F, D>, F: RichField + Extendable<D>, const D: usize> ChipBuilder<L, F, D> {
    pub fn fpmul_const<P: FieldParameters>(
        &mut self,
        a: &FieldRegister<P>,
        c: [u16; MAX_NB_LIMBS],
        result: &FieldRegister<P>,
    ) -> Result<FpMulConst<P>>
    where
        L::Instruction: From<FpMulConst<P>>,
    {
        let carry = self.alloc_local::<FieldRegister<P>>().unwrap();
        let witness_low = self
            .alloc_local_array::<U16Register>(P::NB_WITNESS_LIMBS)
            .unwrap();
        let witness_high = self
            .alloc_local_array::<U16Register>(P::NB_WITNESS_LIMBS)
            .unwrap();
        let instr = FpMulConst {
            a: *a,
            c,
            result: *result,
            carry,
            witness_low,
            witness_high,
        };
        self.insert_instruction(instr.into())?;
        Ok(instr)
    }
}

impl<F: RichField + Extendable<D>, const D: usize, P: FieldParameters> Instruction<F, D>
    for FpMulConst<P>
{
    fn memory_vec(&self) -> Vec<MemorySlice> {
        vec![*self.a.register(), *self.result.register()]
    }

    fn assign_row(&self, trace_rows: &mut [Vec<F>], row: &mut [F], row_index: usize) {
        let mut index = 0;
        self.result
            .register()
            .assign(trace_rows, &mut row[index..P::NB_LIMBS], row_index);
        index += P::NB_LIMBS;
        self.carry
            .register()
            .assign(trace_rows, &mut row[index..index + P::NB_LIMBS], row_index);
        index += P::NB_LIMBS;
        self.witness_low.register().assign(
            trace_rows,
            &mut row[index..index + P::NB_WITNESS_LIMBS],
            row_index,
        );
        index += P::NB_WITNESS_LIMBS;
        self.witness_high.register().assign(
            trace_rows,
            &mut row[index..index + P::NB_WITNESS_LIMBS],
            row_index,
        );
    }

    fn packed_generic_constraints<
        FE,
        PF,
        const D2: usize,
        const COLUMNS: usize,
        const PUBLIC_INPUTS: usize,
    >(
        &self,
        vars: StarkEvaluationVars<FE, PF, { COLUMNS }, { PUBLIC_INPUTS }>,
        yield_constr: &mut crate::constraint_consumer::ConstraintConsumer<PF>,
    ) where
        FE: FieldExtension<D2, BaseField = F>,
        PF: PackedField<Scalar = FE>,
    {
        // get all the data
        let a = self.a.register().packed_entries(&vars);
        let c = self
            .c
            .into_iter()
            .map(FE::from_canonical_u16)
            .map(PF::from)
            .take(P::NB_LIMBS)
            .collect::<Vec<_>>();
        let result = self.result.register().packed_entries(&vars);

        let carry = self.carry.register().packed_entries_slice(&vars);
        let witness_low = self.witness_low.register().packed_entries_slice(&vars);
        let witness_high = self.witness_high.register().packed_entries_slice(&vars);

        // Construct the expected vanishing polynmial
        let ac = PolynomialOps::mul(&a, &c);
        let ac_minus_result = PolynomialOps::sub(&ac, &result);
        let p_limbs = Polynomial::<FE>::from_iter(modulus_field_iter::<FE, P>());
        let mul_times_carry = PolynomialOps::scalar_poly_mul(carry, p_limbs.as_slice());
        let vanishing_poly = PolynomialOps::sub(&ac_minus_result, &mul_times_carry);

        // reconstruct witness
        let limb = FE::from_canonical_u32(LIMB);

        // Reconstruct and shift back the witness polynomial
        let w_shifted = witness_low
            .iter()
            .zip(witness_high.iter())
            .map(|(x, y)| *x + (*y * limb));

        let offset = FE::from_canonical_u32(P::WITNESS_OFFSET as u32);
        let w = w_shifted.map(|x| x - offset).collect::<Vec<PF>>();

        // Multiply by (x-2^16) and make the constraint
        let root_monomial: &[PF] = &[PF::from(-limb), PF::from(PF::Scalar::ONE)];
        let witness_times_root = PolynomialOps::mul(&w, root_monomial);

        //debug_assert!(vanishing_poly.len() == witness_times_root.len());
        for i in 0..vanishing_poly.len() {
            yield_constr.constraint(vanishing_poly[i] - witness_times_root[i]);
        }
    }

    fn ext_circuit_constraints<const COLUMNS: usize, const PUBLIC_INPUTS: usize>(
        &self,
        builder: &mut CircuitBuilder<F, D>,
        vars: StarkEvaluationTargets<D, { COLUMNS }, { PUBLIC_INPUTS }>,
        yield_constr: &mut crate::constraint_consumer::RecursiveConstraintConsumer<F, D>,
    ) {
        // get all the data
        let a = self.a.register().evaluation_targets(&vars);
        let c_vec = self
            .c
            .into_iter()
            .map(F::Extension::from_canonical_u16)
            .take(P::NB_LIMBS)
            .collect::<Vec<_>>();
        let c = PolynomialGadget::constant_extension(builder, &c_vec);
        let result = self.result.register().evaluation_targets(&vars);

        let carry = self.carry.register().evaluation_targets(&vars);
        let witness_low = self.witness_low.register().evaluation_targets(&vars);
        let witness_high = self.witness_high.register().evaluation_targets(&vars);

        // Construct the expected vanishing polynmial
        let ac = PolynomialGadget::mul_extension(builder, a, &c);
        let ac_minus_result = PolynomialGadget::sub_extension(builder, &ac, result);
        let p_limbs = PolynomialGadget::constant_extension(
            builder,
            &modulus_field_iter::<F::Extension, P>().collect::<Vec<_>>(),
        );
        let mul_times_carry = PolynomialGadget::mul_extension(builder, carry, &p_limbs[..]);
        let vanishing_poly =
            PolynomialGadget::sub_extension(builder, &ac_minus_result, &mul_times_carry);

        // reconstruct witness

        // Reconstruct and shift back the witness polynomial
        let limb_const = F::Extension::from_canonical_u32(2u32.pow(16));
        let limb = builder.constant_extension(limb_const);
        let w_high_times_limb =
            PolynomialGadget::ext_scalar_mul_extension(builder, witness_high, &limb);
        let w_shifted = PolynomialGadget::add_extension(builder, witness_low, &w_high_times_limb);
        let offset =
            builder.constant_extension(F::Extension::from_canonical_u32(P::WITNESS_OFFSET as u32));
        let w = PolynomialGadget::sub_constant_extension(builder, &w_shifted, &offset);

        // Multiply by (x-2^16) and make the constraint
        let neg_limb = builder.constant_extension(-limb_const);
        let root_monomial = &[neg_limb, builder.constant_extension(F::Extension::ONE)];
        let witness_times_root =
            PolynomialGadget::mul_extension(builder, w.as_slice(), root_monomial);

        let constraint =
            PolynomialGadget::sub_extension(builder, &vanishing_poly, &witness_times_root);
        for constr in constraint {
            yield_constr.constraint(builder, constr);
        }
    }
}

impl<P: FieldParameters> FpMulConst<P> {
    /// Trace row for fp_mul operation
    ///
    /// Returns a vector
    /// [Input[2 * N_LIMBS], output[N_LIMBS], carry[NUM_CARRY_LIMBS], Witness_low[NUM_WITNESS_LIMBS], Witness_high[NUM_WITNESS_LIMBS]]
    pub fn trace_row<F: RichField + Extendable<D>, const D: usize>(
        &self,
        a: &BigUint,
    ) -> (Vec<F>, BigUint) {
        let p = P::modulus_biguint();
        let mut c = BigUint::zero();
        for (i, limb) in self.c.iter().enumerate() {
            c += BigUint::from(*limb) << (16 * i);
        }
        let result = (a * &c) % &p;
        debug_assert!(result < p);
        let carry = (a * &c - &result) / &p;
        debug_assert!(carry < p);
        debug_assert_eq!(&carry * &p, a * &c - &result);

        // make polynomial limbs
        let p_a = Polynomial::<i64>::from_biguint_num(a, 16, P::NB_LIMBS);
        let p_c = Polynomial::<i64>::from_biguint_num(&c, 16, P::NB_LIMBS);
        let p_p = Polynomial::<i64>::from_biguint_num(&p, 16, P::NB_LIMBS);

        let p_result = Polynomial::<i64>::from_biguint_num(&result, 16, P::NB_LIMBS);
        let p_carry = Polynomial::<i64>::from_biguint_num(&carry, 16, P::NB_LIMBS);

        // Compute the vanishing polynomial
        let vanishing_poly = &p_a * &p_c - &p_result - &p_carry * &p_p;
        debug_assert_eq!(vanishing_poly.degree(), Self::NUM_WITNESS_LOW_LIMBS);

        // Compute the witness
        let witness_shifted = extract_witness_and_shift(&vanishing_poly, P::WITNESS_OFFSET as u32);
        let (witness_low, witness_high) = split_digits::<F>(&witness_shifted);

        let mut row = Vec::with_capacity(Self::num_mul_const_columns());

        // output
        row.extend(to_field_iter::<F>(&p_result));
        // carry and witness
        row.extend(to_field_iter::<F>(&p_carry));
        row.extend(witness_low);
        row.extend(witness_high);

        (row, result)
    }
}

impl<F: RichField + Extendable<D>, const D: usize> TraceHandle<F, D> {
    pub fn write_fpmul_const<P: FieldParameters>(
        &self,
        row_index: usize,
        a_int: &BigUint,
        instruction: FpMulConst<P>,
    ) -> Result<BigUint> {
        let (row, result) = instruction.trace_row::<F, D>(a_int);
        self.write(row_index, instruction, row)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use num::bigint::RandBigInt;
    use plonky2::iop::witness::PartialWitness;
    use plonky2::plonk::circuit_data::CircuitConfig;
    use plonky2::plonk::config::{GenericConfig, PoseidonGoldilocksConfig};
    use plonky2::util::timing::TimingTree;
    //use plonky2_maybe_rayon::*;
    use rand::thread_rng;

    use super::*;
    use crate::arithmetic::builder::ChipBuilder;
    use crate::arithmetic::chip::{ChipParameters, TestStark};
    use crate::arithmetic::field::Fp25519Param;
    use crate::arithmetic::trace::trace;
    use crate::config::StarkConfig;
    use crate::prover::prove;
    use crate::recursive_verifier::{
        add_virtual_stark_proof_with_pis, set_stark_proof_with_pis_target,
        verify_stark_proof_circuit,
    };
    use crate::verifier::verify_stark_proof;

    #[derive(Clone, Debug, Copy)]
    struct FpMulTest;

    impl<F: RichField + Extendable<D>, const D: usize> ChipParameters<F, D> for FpMulTest {
        const NUM_ARITHMETIC_COLUMNS: usize = 124;
        const NUM_FREE_COLUMNS: usize = 0;

        type Instruction = FpMul<Fp25519Param>;
    }

    #[derive(Clone, Debug, Copy)]
    struct FpMulConstTest;

    impl<F: RichField + Extendable<D>, const D: usize> ChipParameters<F, D> for FpMulConstTest {
        const NUM_ARITHMETIC_COLUMNS: usize = FpMulConst::<Fp25519Param>::num_mul_const_columns();
        const NUM_FREE_COLUMNS: usize = 0;

        type Instruction = FpMulConst<Fp25519Param>;
    }

    #[test]
    fn test_fpmul_const() {
        const D: usize = 2;
        type C = PoseidonGoldilocksConfig;
        type F = <C as GenericConfig<D>>::F;
        type Fp = Fp25519;
        type S = TestStark<FpMulConstTest, F, D>;

        let mut c: [u16; MAX_NB_LIMBS] = [0; MAX_NB_LIMBS];
        c[0] = 100;
        c[1] = 2;
        c[2] = 30000;

        let mut c_bigint = BigUint::zero();
        for i in 0..MAX_NB_LIMBS {
            c_bigint += BigUint::from(c[i]) << (i * 16);
        }

        // build the stark
        let mut builder = ChipBuilder::<FpMulConstTest, F, D>::new();

        let a = builder.alloc_local::<Fp>().unwrap();
        let result = builder.alloc_local::<Fp>().unwrap();

        //let ab = FMul::new(a, b, result);
        //builder.insert_instruction(ab).unwrap();
        let ac_ins = builder.fpmul_const(&a, c, &result).unwrap();
        builder.write_data(&a).unwrap();

        let (chip, spec) = builder.build();

        // Construct the trace
        let num_rows = 2u64.pow(16) as usize;
        let (handle, generator) = trace::<F, D>(spec);

        let p = Fp25519Param::modulus_biguint();

        let mut rng = thread_rng();
        for i in 0..num_rows {
            let a_int: BigUint = rng.gen_biguint(256) % &p;
            //let handle = handle.clone();
            //rayon::spawn(move || {
            handle.write_field(i, &a_int, a).unwrap();
            let res = handle.write_fpmul_const(i, &a_int, ac_ins).unwrap();
            assert_eq!(res, (c_bigint.clone() * a_int) % &p);
            //});
        }
        drop(handle);

        let trace = generator.generate_trace(&chip, num_rows).unwrap();

        let config = StarkConfig::standard_fast_config();
        let stark = TestStark::new(chip);

        // Verify proof as a stark
        let proof = prove::<F, C, S, D>(
            stark.clone(),
            &config,
            trace,
            [],
            &mut TimingTree::default(),
        )
        .unwrap();
        verify_stark_proof(stark.clone(), proof.clone(), &config).unwrap();

        // Verify recursive proof in a circuit
        let config_rec = CircuitConfig::standard_recursion_config();
        let mut recursive_builder = CircuitBuilder::<F, D>::new(config_rec);

        let degree_bits = proof.proof.recover_degree_bits(&config);
        let virtual_proof = add_virtual_stark_proof_with_pis(
            &mut recursive_builder,
            stark.clone(),
            &config,
            degree_bits,
        );

        recursive_builder.print_gate_counts(0);

        let mut rec_pw = PartialWitness::new();
        set_stark_proof_with_pis_target(&mut rec_pw, &virtual_proof, &proof);

        verify_stark_proof_circuit::<F, C, S, D>(
            &mut recursive_builder,
            stark,
            virtual_proof,
            &config,
        );

        let recursive_data = recursive_builder.build::<C>();

        let mut timing = TimingTree::new("recursive_proof", log::Level::Debug);
        let recursive_proof = plonky2::plonk::prover::prove(
            &recursive_data.prover_only,
            &recursive_data.common,
            rec_pw,
            &mut timing,
        )
        .unwrap();

        timing.print();
        recursive_data.verify(recursive_proof).unwrap();
    }
}
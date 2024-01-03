use super::BLAKE2BAir;
use crate::chip::register::array::ArrayRegister;
use crate::chip::register::bit::BitRegister;
use crate::chip::register::element::ElementRegister;
use crate::chip::uint::operations::instruction::UintInstructions;
use crate::chip::uint::register::U64Register;
use crate::chip::AirParameters;
use crate::machine::bytes::builder::BytesBuilder;

impl<L: AirParameters> BytesBuilder<L>
    where
        L::Instruction: UintInstructions,
{
    pub fn blake2b(
        &mut self,
        padded_chunks: &[ArrayRegister<U64Register>],
        t_values: &ArrayRegister<U64Register>,
        end_bits: &ArrayRegister<BitRegister>,
        digest_bits: &ArrayRegister<BitRegister>,
        digest_indices: &ArrayRegister<ElementRegister>,
        num_messages: &ElementRegister,
    ) -> Vec<ArrayRegister<U64Register>> {
        BLAKE2BAir::blake2b(
            self,
            padded_chunks,
            t_values,
            end_bits,
            digest_bits,
            digest_indices,
            num_messages,
        )
    }
}

#[cfg(test)]
pub mod test_utils {

    use core::fmt::Debug;

    use itertools::Itertools;
    use log::debug;
    use plonky2::field::goldilocks_field::GoldilocksField;
    use plonky2::iop::witness::{PartialWitness, WitnessWrite};
    use plonky2::plonk::circuit_builder::CircuitBuilder;
    use plonky2::plonk::circuit_data::CircuitConfig;
    use plonky2::timed;
    use plonky2::util::timing::TimingTree;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::chip::uint::operations::instruction::UintInstruction;
    use crate::chip::uint::util::u64_to_le_field_bytes;
    use crate::chip::AirParameters;
    use crate::machine::builder::Builder;
    use crate::machine::bytes::builder::BytesBuilder;
    use crate::machine::hash::blake::blake2b::pure::BLAKE2BPure;
    use crate::machine::hash::blake::blake2b::utils::BLAKE2BUtil;
    use crate::machine::hash::blake::blake2b::IV;
    use crate::math::goldilocks::cubic::GoldilocksCubicParameters;
    use crate::math::prelude::*;
    use crate::plonky2::stark::config::{CurtaConfig, CurtaPoseidonGoldilocksConfig};
    use crate::prelude::{AirWriter, AirWriterData};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct BLAKE2BTest;

    impl AirParameters for BLAKE2BTest {
        type Field = GoldilocksField;
        type CubicParams = GoldilocksCubicParameters;
        type Instruction = UintInstruction;

        const NUM_FREE_COLUMNS: usize = 1527;
        const EXTENDED_COLUMNS: usize = 708;
    }

    #[test]
    pub fn test_blake2b() {
        type C = CurtaPoseidonGoldilocksConfig;
        type Config = <C as CurtaConfig<2>>::GenericConfig;

        let _ = env_logger::builder().is_test(true).try_init();
        let mut timing = TimingTree::new("test_blake2b", log::Level::Info);

        let mut padded_chunks_values = Vec::new();
        let mut t_values_values = Vec::new();
        let mut end_bits_values = Vec::new();
        let mut digest_bits_values = Vec::new();
        let mut digest_indices_values = Vec::new();
        // let num_rows = 1 << 17;
        let num_rows = 512;

        let msgs = [
            // 1 block
            hex::decode("00f43f3ef4c05d1aca645d7b2b59af99d65661810b8a724818052db75e04afb60ea210002f9cac87493604cb5fff6644ea17c3b1817d243bc5a0aa6f0d11ab3df46f37b9adbf1ff3a446807e7a9ebc77647776b8bbda37dcf2f4f34ca7ba7bf4c7babfbe080642414245b501032c000000b7870a0500000000360b79058f3b331fbbb10d38a2e309517e24cc12094d0a5a7c9faa592884e9621aecff0224bc1a857a0bacadf4455e2c5b39684d2d5879b108c98315f6a14504348846c6deed3addcba24fc3af531d59f31c87bc454bf6f1d73eadaf2d22d60c05424142450101eead41c1266af7bc7becf961dcb93f3691642c9b6d50aeb65b92528b99c675608f2095a296ed52aa433c1bfed56e8546dae03b61cb59643a9cb39f82618f958b00041000000000000000000000000000000000000000000000000000000000000000008101a26cc6796f1025d51bd927351af541d3ab01d7a1b978a65e19c16ae2799b3286ca2401211009421c4e6bd80ef9e07918a26cc6796f1025d51bd927351af541d3ab01d7a1b978a65e19c16ae2799b3286ca2401211009421c4e6bd80ef9e079180400").unwrap(),

            // // 1 block
            // hex::decode("092005a6f7a58a98df5f9b8d186b9877f12b603aa06c7debf0f610d5a49f9ed7262b5e095b309af2b0eae1c554e03b6cc4a5a0df207b662b329623f27fdce8d088554d82b1e63bedeb3fe9bd7754c7deccdfe277bcbfad4bbaff6302d3488bd2a8565f4f6e753fc7942fa29051e258da2e06d13b352220b9eadb31d8ead7f88b").unwrap(),

            // // 8 blocks
            // hex::decode("092005a6f7a58a98df5f9b8d186b9877f12b603aa06c7debf0f610d5a49f9ed7262b5e095b309af2b0eae1c554e03b6cc4a5a0df207b662b329623f27fdce8d088554d82b1e63bedeb3fe9bd7754c7deccdfe277bcbfad4bbaff6302d3488bd2a8565f4f6e753fc7942fa29051e258da2e06d13b352220b9eadb31d8ead7f88b244f13c0835db4a3909cee6106b276684aba0f8d8b1b0ba02dff4d659b081adfeab6f3a26d7fd65eff7c72a539dbeee68a9497476b69082958eae7d6a7f0f1d5a1b99a0a349691e80429667831f9b818431514bb2763e26e94a65428d22f3827d491c474c7a1885fe1d2d557e27bbcd81bffa9f3a507649e623b47681d6c9893301d8f635ec49e983cc537c4b81399bb24027ac4be709ce1a4eeb448e98a9aecfe249696419a67cb9e0f29d0297d840048bddf6612a383f37d7b96348a1bc5f1f9ac6eed6eb911dc43e120c8480e0258a6b33e0b91734cc64f144827053b17ae91c62e6866d8b68c1b0e53df0d0f0f4f187278db30c7b95d2741f4d0c8c59507984482b48d356ce8e299268b100c61a9ba5f96a757cf98150683a3e8aa85484a4590b293b6ec62c77f022542a73651a42b50f05a8d10bbb546746ca82221ca3b18105a05e4a7ea9c9d5096a37c8b3ce1a9c62ebd7badd7ee6f1c6e5961a08d066d5e025e08e3ec72531c476098287b13295fa606fab8275418e0c4c54f236c9e73fbfdaa00a5205310cb0d1bd54175647482fae300cc66b36e7846e82288e9f0290d9479d0c1998373900dfb72900d1c9f55c018dd7eeed4ce0e988bb3da03a22910ddec7c51b2eab4d96831a8b9e84a42cebdadae62bdea26ca7b0c640e8a21f86c72277ed20efe15bab1abcf34656e7d2336e42133fa99331e874b5458b28fabe6cb62c4606ee7046d07bc9e5eec2246068396590b59194c10bbe82f7c8b5ddea0d85a4cf74a91c85d7f90873bfbdc40c8c939377bec9a26d66b895a1bbeaa94028d6eafa1c0d6218077d174cc59cea6f2ea17ef1c002160e549f43b03112b0a978fd659c69448273e35554e21bac35458fe2b199f8b8fb81a6488ee99c734e2eefb4dd06c686ca29cdb2173a53ec8322a6cb9128e3b7cdf4bf5a5c2e8906b840bd86fa97ef694a34fd47740c2d44ff7378d773ee090903796a719697e67d8df4bc26d8aeb83ed380c04fe8aa4f23678989ebffd29c647eb96d4999b4a6736dd66c7a479fe0352fda60876f173519b4e567f0a0f0798d25e198603c1c5569b95fefa2edb64720ba97bd4d5f82614236b3a1f5deb344df02d095fccfe1db9b000f38ebe212f804ea0fbbeb645b8375e21d27f5381de0e0c0156f2fa3a0a0a055b8afe90b542f6e0fffb744f1dba74e34bb4d3ea6c84e49796f5e549781a2f5c2dc01d7b8e814661b5e2d2a51a258b2f7032a83082e6e36a5e51ef9af960b058").unwrap(),

            // // 8 blocks
            // hex::decode("092005a6f7a58a98df5f9b8d186b9877f12b603aa06c7debf0f610d5a49f9ed7262b5e095b309af2b0eae1c554e03b6cc4a5a0df207b662b329623f27fdce8d088554d82b1e63bedeb3fe9bd7754c7deccdfe277bcbfad4bbaff6302d3488bd2a8565f4f6e753fc7942fa29051e258da2e06d13b352220b9eadb31d8ead7f88b244f13c0835db4a3909cee6106b276684aba0f8d8b1b0ba02dff4d659b081adfeab6f3a26d7fd65eff7c72a539dbeee68a9497476b69082958eae7d6a7f0f1d5a1b99a0a349691e80429667831f9b818431514bb2763e26e94a65428d22f3827d491c474c7a1885fe1d2d557e27bbcd81bffa9f3a507649e623b47681d6c9893301d8f635ec49e983cc537c4b81399bb24027ac4be709ce1a4eeb448e98a9aecfe249696419a67cb9e0f29d0297d840048bddf6612a383f37d7b96348a1bc5f1f9ac6eed6eb911dc43e120c8480e0258a6b33e0b91734cc64f144827053b17ae91c62e6866d8b68c1b0e53df0d0f0f4f187278db30c7b95d2741f4d0c8c59507984482b48d356ce8e299268b100c61a9ba5f96a757cf98150683a3e8aa85484a4590b293b6ec62c77f022542a73651a42b50f05a8d10bbb546746ca82221ca3b18105a05e4a7ea9c9d5096a37c8b3ce1a9c62ebd7badd7ee6f1c6e5961a08d066d5e025e08e3ec72531c476098287b13295fa606fab8275418e0c4c54f236c9e73fbfdaa00a5205310cb0d1bd54175647482fae300cc66b36e7846e82288e9f0290d9479d0c1998373900dfb72900d1c9f55c018dd7eeed4ce0e988bb3da03a22910ddec7c51b2eab4d96831a8b9e84a42cebdadae62bdea26ca7b0c640e8a21f86c72277ed20efe15bab1abcf34656e7d2336e42133fa99331e874b5458b28fabe6cb62c4606ee7046d07bc9e5eec2246068396590b59194c10bbe82f7c8b5ddea0d85a4cf74a91c85d7f90873bfbdc40c8c939377bec9a26d66b895a1bbeaa94028d6eafa1c0d6218077d174cc59cea6f2ea17ef1c002160e549f43b03112b0a978fd659c69448273e35554e21bac35458fe2b199f8b8fb81a6488ee99c734e2eefb4dd06c686ca29cdb2173a53ec8322a6cb9128e3b7cdf4bf5a5c2e8906b840bd86fa97ef694a34fd47740c2d44ff7378d773ee090903796a719697e67d8df4bc26d8aeb83ed380c04fe8aa4f23678989ebffd29c647eb96d4999b4a6736dd66c7a479fe0352fda60876f173519b4e567f0a0f0798d25e198603c1c5569b95fefa2edb64720ba97bd4d5f82614236b3a1f5deb344df02d095fccfe1db9b000f38ebe212f804ea0fbbeb645b8375e21d27f5381de0e0c0156f2fa3a0a0a055b8afe90b542f6e0fffb744f1dba74e34bb4d3ea6c84e49796f5e549781a2f5c2dc01d7b8e814661b5e2d2a51a258b2f7032a83082e6e36a5e51").unwrap(),
        ];
        // let msg_max_chunk_sizes = [4u64, 4, 35, 35];
        let msg_max_chunk_sizes = [4u64];

        let mut start_index = 0;
        // for _i in 0..17 {
        for _i in 0..1 {
            for (msg, msg_max_chunk_size) in msgs.iter().zip_eq(msg_max_chunk_sizes.iter()) {
                let msg_u64_limbs: Vec<[GoldilocksField; 8]> =
                    BLAKE2BUtil::pad(msg, *msg_max_chunk_size)
                        .chunks_exact(8)
                        .map(|x| {
                            x.iter()
                                .map(|y| GoldilocksField::from_canonical_u8(*y))
                                .collect_vec()
                                .try_into()
                                .unwrap()
                        })
                        .collect_vec();

                let msg_padded_chunks: Vec<[[GoldilocksField; 8]; 16]> = msg_u64_limbs
                    .chunks_exact(16)
                    .map(|x| x.try_into().unwrap())
                    .collect_vec();

                let mut t_value = 0u64;
                let msg_len = msg.len();
                let msg_digest_idx = if msg_len == 0 { 0 } else { (msg_len - 1) / 128 };
                assert!(msg_padded_chunks.len() == *msg_max_chunk_size as usize);
                for (i, chunk) in msg_padded_chunks.iter().enumerate() {
                    padded_chunks_values.push(*chunk);

                    t_value += 128;

                    let at_digest_chunk = i == msg_digest_idx;
                    t_values_values.push(if at_digest_chunk {
                        msg_len as u64
                    } else {
                        t_value
                    });

                    digest_bits_values.push(GoldilocksField::from_canonical_usize(
                        at_digest_chunk as usize,
                    ));
                    if at_digest_chunk {
                        digest_indices_values.push(GoldilocksField::from_canonical_usize(
                            start_index + msg_digest_idx,
                        ));
                    }

                    end_bits_values.push(GoldilocksField::from_canonical_usize(
                        (i == msg_padded_chunks.len() - 1) as usize,
                    ));
                }

                start_index += msg_padded_chunks.len();
            }
        }

        // let num_messages_value = GoldilocksField::from_canonical_usize(17 * msgs.len());
        let num_messages_value = GoldilocksField::from_canonical_usize(msgs.len());

        // Build the stark
        let num_rounds = padded_chunks_values.len();
        let mut builder = BytesBuilder::<BLAKE2BTest>::new();
        let padded_chunks = (0..num_rounds)
            .map(|_| builder.alloc_array_public::<U64Register>(16))
            .collect::<Vec<_>>();
        let t_values = builder.alloc_array_public::<U64Register>(num_rounds);
        let end_bits = builder.alloc_array_public::<BitRegister>(num_rounds);
        let digest_bits = builder.alloc_array_public::<BitRegister>(num_rounds);
        // let digest_indices = builder.alloc_array_public(17 * msgs.len());
        let digest_indices = builder.alloc_array_public(msgs.len());
        let num_messages = builder.alloc_public();
        let hash_state = builder.blake2b(
            &padded_chunks,
            &t_values,
            &end_bits,
            &digest_bits,
            &digest_indices,
            &num_messages,
        );

        let stark = builder.build::<C, 2>(num_rows);

        let config_rec = CircuitConfig::standard_recursion_config();
        let mut recursive_builder = CircuitBuilder::<GoldilocksField, 2>::new(config_rec);

        let (proof_target, public_input) =
            stark.add_virtual_proof_with_pis_target(&mut recursive_builder);
        stark.verify_circuit(&mut recursive_builder, &proof_target, &public_input);

        let rec_data = recursive_builder.build::<Config>();

        // Write trace.
        let mut writer_data = AirWriterData::new(&stark.air_data, num_rows);
        let mut writer = writer_data.public_writer();

        writer.write(&num_messages, &num_messages_value);
        let mut hash_state_iter = hash_state.iter();
        let mut current_state = IV;
        for i in 0..num_rounds {
            let padded_chunk = padded_chunks_values[i];
            writer.write_array(&padded_chunks[i], padded_chunk);
            writer.write(&end_bits.get(i), &end_bits_values[i]);
            writer.write(&digest_bits.get(i), &digest_bits_values[i]);
            writer.write(&t_values.get(i), &u64_to_le_field_bytes(t_values_values[i]));

            let chunk = padded_chunks_values[i];
            BLAKE2BPure::compress(
                &chunk
                    .iter()
                    .flatten()
                    .map(|x| GoldilocksField::as_canonical_u64(x) as u8)
                    .collect_vec(),
                &mut current_state,
                t_values_values[i],
                digest_bits_values[i] == GoldilocksField::ONE,
            );

            if digest_bits_values[i] == GoldilocksField::ONE {
                writer.write_array(
                    hash_state_iter.next().unwrap(),
                    current_state[0..4]
                        .iter()
                        .map(|x| u64_to_le_field_bytes(*x)),
                );
            }

            if end_bits_values[i] == GoldilocksField::ONE {
                current_state = IV;
            }
        }

        for (i, digest_index) in digest_indices_values.iter().enumerate() {
            writer.write(&digest_indices.get(i), digest_index);
        }

        timed!(timing, log::Level::Info, "write input", {
            stark.air_data.write_global_instructions(&mut writer);

            for mut chunk in writer_data.chunks(num_rows) {
                for i in 0..num_rows {
                    debug!("writing trace instructions for row {}", i);
                    let mut writer = chunk.window_writer(i);
                    stark.air_data.write_trace_instructions(&mut writer);
                }
            }
        });

        let (trace, public) = (writer_data.trace, writer_data.public);

        let proof = timed!(
            timing,
            log::Level::Info,
            "generate stark proof",
            stark.prove(&trace, &public, &mut timing).unwrap()
        );

        stark.verify(proof.clone(), &public).unwrap();

        let mut pw = PartialWitness::new();

        pw.set_target_arr(&public_input, &public);
        stark.set_proof_target(&mut pw, &proof_target, proof);

        let rec_proof = timed!(
            timing,
            log::Level::Info,
            "generate recursive proof",
            rec_data.prove(pw).unwrap()
        );
        rec_data.verify(rec_proof).unwrap();

        timing.print();
    }
}

use serde::{Deserialize, Serialize};

use super::value::ByteOperation;
use crate::air::parser::AirParser;
use crate::air::AirConstraint;
use crate::chip::instruction::Instruction;
use crate::chip::register::element::ElementRegister;
use crate::chip::trace::writer::{AirWriter, TraceWriter};
use crate::chip::uint::bytes::register::ByteRegister;
use crate::math::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByteOperationInstruction {
    inner: ByteOperation<ByteRegister>,
    digest: ElementRegister,
    global: bool,
}

impl ByteOperationInstruction {
    pub fn new(inner: ByteOperation<ByteRegister>, digest: ElementRegister, global: bool) -> Self {
        ByteOperationInstruction {
            inner,
            digest,
            global,
        }
    }
}

impl<AP: AirParser> AirConstraint<AP> for ByteOperationInstruction {
    fn eval(&self, parser: &mut AP) {
        self.inner.lookup_digest_constraint(parser, self.digest);
    }
}

impl<F: PrimeField64> Instruction<F> for ByteOperationInstruction {
    fn write(&self, writer: &TraceWriter<F>, row_index: usize) {
        if self.global && row_index != 0 {
            return;
        }
        let value = self.inner.write(writer, row_index);
        let digest = F::from_canonical_u32(value.lookup_digest_value());
        writer.write(&self.digest, &digest, row_index);
    }

    fn write_to_air(&self, writer: &mut impl AirWriter<Field = F>) {
        if let Some(r) = writer.row_index() {
            if self.global && r != 0 {
                return;
            }
        }
        let value = self.inner.write_to_air(writer);
        let digest = F::from_canonical_u32(value.lookup_digest_value());
        writer.write(&self.digest, &digest);
    }
}

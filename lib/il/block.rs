//! A `Block` is a linear sequences of `Instruction`.
//!
//! A `Block` must belong to a `ControlFlowGraph`. A `Block` contains many `Instruction`.
//!
//! When building a series of `Operation`/`Instruction`, we normally do so by calling the relevant
//! method directly on the block where we wish to add the `Instruction`.
//!
//! To create a `Block`, call `ControlFlowGraph::new_block`.

use std::fmt;
use il::*;


/// A basic block in Falcon IL.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Block {
    /// The index of the block.
    index: u64,
    /// an internal counter for the next block-unique instruction.
    next_instruction_index: u64,
    /// An internal counter for the next block-unique temporary variable.
    next_temp_index: u64,
    /// The instructions for this block.
    instructions: Vec<Instruction>,
}


impl Block {
    pub(crate) fn new(index: u64) -> Block {
        Block {
            index: index,
            next_instruction_index: 0,
            next_temp_index: 0,
            instructions: Vec::new()
        }
    }


    fn new_instruction_index(&mut self) -> u64 {
        let instruction_index = self.next_instruction_index;
        self.next_instruction_index = instruction_index + 1;
        instruction_index
    }


    fn push(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }


    /// Appends the contents of another `Block` to this `Block`.
    ///
    /// Instruction indices are updated accordingly.
    pub fn append(&mut self, other: &Block) {
        for instruction in other.instructions().iter() {
            let instruction = instruction.clone_new_index(self.new_instruction_index());
            self.instructions.push(instruction);
        }
    }


    /// Returns the index of this `Block`
    pub fn index(&self) -> u64 {
        self.index
    }


    /// Returns instructions for this `Block`
    pub fn instructions(&self) -> &Vec<Instruction> {
        &self.instructions
    }


    /// Returns a mutable reference to the instructions for this `Block`.
    pub fn instructions_mut(&mut self) -> &mut Vec<Instruction> {
        &mut self.instructions
    }


    /// Returns an `Instruction` by index, or `None` if the instruction does not
    /// exist.
    pub fn instruction(&self, index: u64) -> Option<&Instruction> {
        for instruction in &self.instructions {
            if instruction.index() == index {
                return Some(instruction);
            }
        }
        None
    }


    /// Returns a mutable reference to an `Instruction` by index, or `None` if
    /// the `Instruction` does not exist.
    pub fn instruction_mut<>(&mut self, index: u64) -> Option<&mut Instruction> {
        for i in 0..self.instructions.len() {
            if self.instructions[i].index() == index {
                return Some(&mut self.instructions[i]);
            }
        }
        None
    }


    /// Deletes an `Instruction` by its index.
    pub fn remove_instruction(&mut self, index: u64) -> Result<()> {
        let mut vec_index = None;
        for i in 0..self.instructions.len() {
            if self.instructions[i].index() == index {
                vec_index = Some(i);
                break;
            }
        }
        match vec_index {
            Some(index) => {
                self.instructions.remove(index);
                Ok(())
            },
            None => Err(format!("No instruction with index {} found", index).into()),
        }
    }


    /// Clone this block and set a new index.
    pub(crate) fn clone_new_index(&self, index: u64) -> Block {
        let mut clone = self.clone();
        clone.index = index;
        clone
    }


    /// Generates a temporary scalar unique to this block.
    pub fn temp(&mut self, bits: usize) -> Scalar {
        let next_index = self.next_temp_index;
        self.next_temp_index = next_index + 1;
        Scalar::new(format!("temp_{}.{}", self.index, next_index), bits)
    }

    /// Adds an assign operation to the end of this block.
    pub fn assign(&mut self, dst: Scalar, src: Expression) {
        let index = self.new_instruction_index();
        self.push(Instruction::assign(index, dst, src));
    }

    /// Adds a store operation to the end of this block.
    pub fn store(&mut self, dst: Array, address: Expression, src: Expression) {
        let index = self.new_instruction_index();
        self.push(Instruction::store(index, dst, address, src))
    }

    /// Adds a load operation to the end of this block.
    pub fn load(&mut self, dst: Scalar, address: Expression, src: Array) {
        let index = self.new_instruction_index();
        self.push(Instruction::load(index, dst, address, src));
    }

    /// Adds a conditional branch operation to the end of this block.
    pub fn brc(&mut self, dst: Expression, condition: Expression) {
        let index = self.new_instruction_index();
        self.push(Instruction::brc(index, dst, condition));
    }

    /// Adds a phi operation to the end of this block.
    pub fn phi(&mut self, dst: MultiVar, src: Vec<MultiVar>) {
        let index = self.new_instruction_index();
        self.push(Instruction::phi(index, dst, src));
    }

    /// Adds a raise operation to the end of this block.
    pub fn raise(&mut self, expr: Expression) {
        let index = self.new_instruction_index();
        self.push(Instruction::raise(index, expr));
    }

    /// Prepends an operation to the beginning of this block
    pub fn prepend_phi(&mut self, dst: MultiVar, src: Vec<MultiVar>) {
        let index = self.new_instruction_index();
        let phi = Instruction::phi(index, dst, src);
        self.instructions.insert(0, phi);
    }
}


impl graph::Vertex for Block {
    fn index (&self) -> u64 { self.index }
    fn dot_label(&self) -> String { format!("{}", self) }
}


impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        try!(writeln!(f, "[ Block: 0x{:X} ]", self.index));
        for instruction in self.instructions() {
            try!(writeln!(f, "{}", instruction));
        }
        Ok(())
    }
}
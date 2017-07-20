use error::*;
use goblin;
use goblin::Hint;
use loader::*;
use loader::memory::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

// http://stackoverflow.com/questions/37678698/function-to-build-a-fixed-sized-array-from-slice/37679019#37679019
use std::convert::AsMut;

fn clone_into_array<A, T>(slice: &[T]) -> A
    where A: Sized + Default + AsMut<[T]>,
          T: Clone
{
    let mut a = Default::default();
    <A as AsMut<[T]>>::as_mut(&mut a).clone_from_slice(slice);
    a
}


/// The address where the first library will be loaded
const DEFAULT_LIB_BASE: u64 = 0x80000000;
/// The step in address between where we will load libraries.
const LIB_BASE_STEP: u64    = 0x04000000;


// Loads and links multiple ELFs together
#[derive(Clone, Debug)]
pub struct ElfLinker {
    /// The filename (path included) of the file we're loading.
    filename: PathBuf,
    /// A mapping from lib name (for example `libc.so.6`) to Elf.
    loaded: BTreeMap<String, Elf>,
    /// The current memory mapping.
    memory: Memory,
    /// The address we will place the next library at.
    next_lib_address: u64
}


impl ElfLinker {
    pub fn new(filename: &Path) -> Result<ElfLinker> {
        let mut elf_linker = ElfLinker {
            filename: filename.to_owned(),
            loaded: BTreeMap::new(),
            memory: Memory::new(),
            next_lib_address: DEFAULT_LIB_BASE
        };

        elf_linker.load_elf(filename, 0)?;

        Ok(elf_linker)
    }


    pub fn load_elf(&mut self, filename: &Path, base_address: u64)
        -> Result<()> {

        // Does this file exist in the same directory as the original file?
        let mut base_path = match self.filename.as_path().parent() {
            Some(base_path) => base_path.to_path_buf(),
            None => PathBuf::new()
        };
        base_path.push(filename);

        let filename = if base_path.exists() {
            &base_path
        }
        else {
            filename
        };
        
        info!("Loading {} with base_address 0x{:x}",
            filename.to_str().unwrap(),
            base_address);
        let elf = Elf::from_file_with_base_address(filename, base_address)?;


        // Update our memory map based on what's in the Elf
        for segment in elf.memory()?.segments() {
            self.memory.add_segment(segment.1.clone());
        }

        // Add this Elf to the loaded Elfs
        let filename = filename.file_name()
                               .unwrap()
                               .to_str()
                               .unwrap()
                               .to_string();
        self.loaded.insert(filename.clone(), elf);

        // Ensure all shared objects we rely on are loaded
        for so_name in self.loaded[&filename].dt_needed()?.clone() {
            if self.loaded.get(&so_name).is_none() {
                self.next_lib_address += LIB_BASE_STEP;
                let next_lib_address = self.next_lib_address;
                self.load_elf(Path::new(&so_name), next_lib_address)?;
            }
        }

        Ok(())

        // Process relocations
    }
}


impl Loader for ElfLinker {
    fn memory(&self) -> Result<memory::Memory> {
        Ok(self.memory.clone())
    }

    fn function_entries(&self) -> Result<Vec<FunctionEntry>> {
        let mut function_entries = Vec::new();
        for loaded in &self.loaded {
            // let fe = loaded.1.function_entries()?;
            // for e in &fe {
            //     println!("{} 0x{:x}", loaded.0, e.address());
            // }
            function_entries.append(&mut loaded.1.function_entries()?);
        }
        Ok(function_entries)
    }

    // TODO Just maybe a bit too much unwrapping here.
    fn program_entry(&self) -> u64 {
        let filename = self.filename
                           .as_path()
                           .file_name()
                           .unwrap()
                           .to_str()
                           .unwrap();
        return self.loaded[filename].program_entry();
    }

    fn architecture(&self) -> Result<Architecture> {
        let filename = self.filename
                           .as_path()
                           .file_name()
                           .unwrap()
                           .to_str()
                           .unwrap();
        return self.loaded[filename].architecture();
    }
}



#[derive(Clone, Debug)]
pub struct ElfSymbol {
    name: String,
    address: u64
}


impl ElfSymbol {
    pub fn new<S: Into<String>>(name: S, address: u64) -> ElfSymbol {
        ElfSymbol {
            name: name.into(),
            address: address
        }
    }


    pub fn name(&self) -> &str {
        &self.name
    }


    pub fn address(&self) -> u64 {
        self.address
    }
}



#[derive(Clone, Debug)]
pub struct Elf {
    base_address: u64,
    bytes: Vec<u8>,
    user_function_entries: Vec<u64>
}


impl Elf {
    pub fn new(bytes: Vec<u8>, base_address: u64) -> Result<Elf> {
        let peek_bytes: [u8; 16] = clone_into_array(&bytes[0..16]);
        // Load this Elf

        let elf = match goblin::peek_bytes(&peek_bytes)? {
            Hint::Elf(_) => Elf {
                base_address: base_address,
                bytes: bytes,
                user_function_entries: Vec::new()
            },
            _ => return Err("Not a valid elf".into())
        };

        Ok(elf)
    }

    /// Get the base address of this Elf where it has been loaded into loader
    /// memory.
    pub fn base_address(&self) -> u64 {
        self.base_address
    }


    /// Load an Elf from a file and use the given base address.
    pub fn from_file_with_base_address(filename: &Path, base_address: u64)
        -> Result<Elf> {

        let mut file = match File::open(filename) {
            Ok(file) => file,
            Err(e) => return Err(format!(
                "Error opening {}: {}",
                filename.to_str().unwrap(),
                e).into())
        };
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Elf::new(buf, base_address)
    }

    /// Load an elf from a file and use the base address of 0.
    pub fn from_file(filename: &Path) -> Result<Elf> {
        Elf::from_file_with_base_address(filename, 0)
    }

    // Allow the user to manually specify a function entry
    pub fn add_user_function(&mut self, address: u64) {
        self.user_function_entries.push(address);
    }

    /// Return the strings from the DT_NEEDED entries.
    pub fn dt_needed(&self) -> Result<Vec<String>> {
        let mut v = Vec::new();

        let memory = self.memory()?;

        let elf = self.elf();
        if let Some(dynamic) = elf.dynamic {
            // We need that strtab, and we have to do this one manually.
            // Get the strtab address
            let mut strtab_address = None;
            for dyn in &dynamic.dyns {
                if dyn.d_tag == goblin::elf::dyn::DT_STRTAB {
                    strtab_address = Some(dyn.d_val);
                    break;
                }
            }
            if strtab_address.is_none() {
                return Ok(v);
            }
            let strtab_address = strtab_address.unwrap();
            // We're going to make a pretty safe assumption that strtab is all
            // in one section
            let mut strtab = None;
            for section_header in &elf.section_headers {
                if    section_header.sh_addr <= strtab_address
                   && section_header.sh_addr + section_header.sh_size > strtab_address {
                    let start = section_header.sh_offset + (strtab_address - section_header.sh_addr);
                    let size = section_header.sh_size - (start - section_header.sh_offset);
                    let start = start as usize;
                    let size = size as usize;
                    let strtab_bytes = self.bytes.get(start..(start + size)).unwrap();
                    strtab = Some(goblin::strtab::Strtab::new(strtab_bytes.to_vec(), 0));
                }
            }
            if strtab.is_none() {
                panic!("Failed to get Dynamic strtab");
            }
            let strtab = strtab.unwrap();

            for dyn in dynamic.dyns {
                if dyn.d_tag == goblin::elf::dyn::DT_NEEDED {
                    let so_name = strtab.get(dyn.d_val as usize);
                    info!("Adding {} from DT_NEEDED", so_name);
                    v.push(so_name.to_string());
                }
            }
        }

        Ok(v)
    }

    /// Return the goblin::elf::Elf for this elf.
    fn elf(&self) -> goblin::elf::Elf {
        goblin::elf::Elf::parse(&self.bytes).unwrap()
    }

    /// Return all symbols exported from this Elf
    pub fn exported_functions(&self) -> Vec<ElfSymbol> {
        let mut v = Vec::new();
        let elf = self.elf();
        for sym in elf.dynsyms {
            if ! sym.is_function() || sym.st_shndx == 0 {
                continue;
            }

            v.push(ElfSymbol::new(elf.dynstrtab.get(sym.st_name), sym.st_value));
        }

        v
    }
}



impl Loader for Elf {
    fn memory(&self) -> Result<Memory> {
        let elf = self.elf();
        let mut memory = Memory::new();

        for ph in elf.program_headers {
            if ph.p_type == goblin::elf::program_header::PT_LOAD {
                let file_range = (ph.p_offset as usize)..((ph.p_offset + ph.p_filesz) as usize);
                let mut bytes = self.bytes
                                    .get(file_range)
                                    .ok_or("Malformed Elf")?
                                    .to_vec();

                if bytes.len() != ph.p_memsz as usize {
                    bytes.append(&mut vec![0; (ph.p_memsz - ph.p_filesz) as usize]);
                }

                let mut permissions = NONE;
                if ph.p_flags & goblin::elf::program_header::PF_R != 0 {
                    permissions |= READ;
                }
                if ph.p_flags & goblin::elf::program_header::PF_W != 0 {
                    permissions |= WRITE;
                }
                if ph.p_flags & goblin::elf::program_header::PF_X != 0 {
                    permissions |= EXECUTE;
                }
                
                let segment = MemorySegment::new(
                    ph.p_vaddr + self.base_address,
                    bytes,
                    permissions
                );

                memory.add_segment(segment);
            }
        }

        Ok(memory)
    }


    fn function_entries(&self) -> Result<Vec<FunctionEntry>> {
        let elf = self.elf();

        let mut function_entries = Vec::new();

        let mut functions_added: BTreeSet<u64> = BTreeSet::new();

        // dynamic symbols
        for sym in &elf.dynsyms {
            if sym.is_function() && sym.st_value != 0 {
                let name = elf.dynstrtab.get(sym.st_name).to_string();
                function_entries.push(FunctionEntry::new(
                    sym.st_value + self.base_address,
                    Some(name)
                ));
                functions_added.insert(sym.st_value);
            }
        }

        // normal symbols
        for sym in &elf.syms {
            if sym.is_function() && sym.st_value != 0 {
                let name = elf.strtab.get(sym.st_name).to_string();
                println!("found function symbol {} at {:x}", name, sym.st_value);
                function_entries.push(FunctionEntry::new(
                    sym.st_value + self.base_address,
                    Some(name))
                );
                functions_added.insert(sym.st_value);
            }
        }


        if !functions_added.contains(&elf.header.e_entry) {
            function_entries.push(FunctionEntry::new(
                elf.header.e_entry + self.base_address,
                None
            ));
        }

        for user_function_entry in &self.user_function_entries {
            if functions_added.get(&(user_function_entry + self.base_address)).is_some() {
                continue;
            }

            function_entries.push(FunctionEntry::new(
                user_function_entry + self.base_address,
                Some(format!("user_function_{:x}", user_function_entry))
            ));
        }

        Ok(function_entries)
    }


    fn program_entry(&self) -> u64 {
        self.elf().header.e_entry
    }


    fn architecture(&self) -> Result<Architecture> {
        let elf = self.elf();

        if elf.header.e_machine == goblin::elf::header::EM_386 {
            Ok(Architecture::X86)
        }
        else {
            Err("Unsupported Arcthiecture".into())
        }
    }
}

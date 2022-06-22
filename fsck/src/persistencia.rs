use fuse::{FileAttr}; //Libreria para el manejo del FileSytem en User Space
use std::str; // Libreria estandar de string
use std::mem; //Libreria estandar para el manejo de memoria
use std::fs::File; // Liberia para el manejo de archivos
use std::io::prelude::*; //Libreria para el manejo de entradas y salidas 
use std::path::Path; // Libreria para obtener el current path del proyecto
use std::fs::OpenOptions; //Libreria asistente para el manejo de los archivos.
use serde::{Serialize, Deserialize}; //Libreria para el manejo de serealizacion
use crate::serialization::FileAttrDef; //Libreria propietaria del proyecto
use bincode::{serialize, deserialize}; //Libreria para encodificar y codificar en binario
use fuse::{FileType};//Libreria para el manejo del FileSytem en User Space

big_array! { BigArray; }
// Estructura para el disco virtual
#[derive(Debug)]
#[allow(dead_code)]
pub struct Disk {
    pub super_block: Box<[Option<Inode>]>,
    pub memory_blocks: Box<[MemoryBlock]>,
    pub max_files: usize,
    pub block_size: usize,
    pub root_path: String,
    phrase: String
}
// Estructura de los i-nodes
#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct Inode {
    #[serde(with = "BigArray")]
    pub name: [char; 64],
    #[serde(with = "FileAttrDef")]
    pub attributes: FileAttr,
    #[serde(with = "BigArray")]
    pub references: [Option<usize>; 128]
}
#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct MemoryBlock {
    data: Option<Box<[u8]>>
}

impl Disk {

    /// Inicializa un disco virtual con el tamaño total especificado en `memory_size_in_bytes` y cada bloque contiene un tamaño fijo definido en `block_size`.
    /// El número de bloques asignados se define mediante la expresión `memory_size_in_bytes / block_size`.
    pub fn new(
        root_path: String,
        memory_size_in_bytes: usize,
        block_size: usize,
        phrase: String
    ) -> Disk {
        // Número de bloques de memoria
        // El -1 se refiere al "superbloque", que tiene el mismo tamaño que un MemoryBlock
        let memory_block_quantity: usize = (memory_size_in_bytes / block_size) - 1;
        // Se está considerando el tamaño del puntero Box además del tamaño de la estructura Inode
        let inode_size = mem::size_of::<Box<[Inode]>>() + mem::size_of::<Inode>();
        let max_files = block_size / inode_size;

        let disk_file_path = format!("{}/disco.qrfs", &root_path);
        let inode_table_file_path = format!("{}/inode.qrfs", &root_path);

        // Intente leer el archivo del disco, si no existe, crea uno nuevo
        let mut memory_blocks: Vec<MemoryBlock>;
        let mut super_block: Vec<Option<Inode>>;

        if Path::new(&disk_file_path).exists() && Path::new(&inode_table_file_path).exists() {
            println!("¡Disco existente encontrado! Cargando...");

            let mut ser_inodes: Vec<u8> = Vec::new();
            let mut ser_disk: Vec<u8> = Vec::new();

            File::open(&inode_table_file_path).unwrap().read_to_end(&mut ser_inodes).unwrap();
            File::open(&disk_file_path).unwrap().read_to_end(&mut ser_disk).unwrap();

            super_block = if &ser_inodes.len() > &0 {
                deserialize(&ser_inodes).expect("¡Error al leer el disco persistido!")
             } else {
                Vec::new()
            };

            memory_blocks = if &ser_disk.len() > &0 {
                deserialize(&ser_disk).expect("¡Error al leer el disco persistido!")
            } else {
                Vec::new()
            };

            // Si la cantidad de bloques en MemoryBlockel disco existente es mayor que la del disco a crear, la ejecución finaliza
            if memory_block_quantity < memory_blocks.len() {
                panic!("¡El disco existente es más grande que el disco actual! ¡Intenta arrancar con un disco de mayor tamaño!");
            }
        } else {
            File::create(&disk_file_path).expect("¡Error al crear archivos para la persistencia!");
            File::create(&inode_table_file_path).expect("¡Error al crear archivos para la persistencia!");

            super_block = Vec::with_capacity(1);
            memory_blocks = Vec::new();

            let ts = time::now().to_timespec();
            let attr = FileAttr {
                ino: 1,
                size: 0,
                blocks: 0,
                atime: ts,
                mtime: ts,
                ctime: ts,
                crtime: ts,
                kind: FileType::Directory,
                perm: 0o755,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            };

            let mut name = ['\0'; 64];
            name[0] = '.';

            let initial_inode = Inode {
                name,
                attributes: attr,
                references: [None; 128]
            };

            super_block.push(Some(initial_inode));
        };

        // Instanciando en blanco otras posiciones posibles para una mayor velocidad
        for _ in super_block.len()..max_files {
            let value: Option<Inode> = Option::None;
            super_block.push(value);
        }

        for _ in memory_blocks.len()..memory_block_quantity {
            let value: MemoryBlock = MemoryBlock { data: Option::None };
            memory_blocks.push(value);
        }

        println!("\nTamaño del disco: {} KB", memory_size_in_bytes / 1024);
        println!("Tamaño del bloque de memoria {} KB", block_size / 1024);
        println!("Número máximo de archivos (Inode {} bytes): {}", inode_size, max_files);
        //println!("{:?}",memory_blocks);
        Disk {
            memory_blocks: memory_blocks.into_boxed_slice(),
            super_block: super_block.into_boxed_slice(),
            max_files,
            block_size,
            root_path,
            phrase
        }
    }

    /// Busca el vector `super_block` para un espacio de memoria vacío (con `None`) y devuelve el número `ino` disponible, si lo hay.
    /// Por convención, el número de inodo `ino` es el número del índice que ocupa en el vector `super_block` + 1.
    pub fn find_ino_available(&self) -> Option<u64> {
        for index in 0..self.super_block.len() - 1 {
            if let Option::None = self.super_block[index] {
                let ino = (index as u64) + 1;
                return Option::Some(ino);
            }
        }

        Option::None
    }

    /// Busca en la matriz `memory_blocks` un espacio de memoria vacío (con `None`) y devuelve el índice de bloque, si lo hay.
    pub fn find_index_of_empty_memory_block(&self) -> Option<usize> {
        for index in 0..self.memory_blocks.len() - 1 {
            if let Option::None = self.memory_blocks[index].data {
                return Option::Some(index);
            }
        }

        Option::None
    }

    /// Funcion que busca el vector `referencias` de un inodo identificado por su número `ino` el primer espacio vacío y devuelve su índice.
    pub fn find_index_of_empty_reference_in_inode(&self, ino: u64) -> Option<usize> {
        let index = (ino as usize) - 1;
        match &self.super_block[index] {
            Some(inode) => inode.references.iter().position(|r| r == &None),
            None => panic!("Intento de accesso a memoria inválido")
        }
    }

    /// Funcion que guarda el `inodo` en el vector `super_block`. Si el número de Inodo `ino` ya existe, los datos se sobrescriben.
    pub fn write_inode(&mut self, inode: Inode) {
        if mem::size_of_val(&inode) > self.block_size {
            println!("No se puede guardar el inodo: tamaño mayor que el tamaño del bloque de memoria");
            return;
        }

        let index = (inode.attributes.ino - 1) as usize;
        self.super_block[index] = Some(inode);
    }

    pub fn clear_memory_block(&mut self, index: usize) {
        self.memory_blocks[index] = MemoryBlock { data: None };
    }

    pub fn clear_inode(&mut self, ino: u64) {
        let index = (ino - 1) as usize;
        self.super_block[index] = None;
    }

    /// Funcion que elimina la referencia del vector de referencias de un Inodo
    pub fn clear_reference_in_inode(&mut self, ino: u64, ref_value: usize) {
        let index = (ino - 1) as usize;
        let inode: &mut Option<Inode> = &mut self.super_block[index];
        
        match inode {
            Some(inode) => {
                let reference_index: Option<usize> = inode.references.iter().position(|r| match r {
                    Some(reference) => *reference == ref_value,
                    None => false
                });

                match reference_index {
                    Some(reference_index) => inode.references[reference_index] = None,
                    None => panic!("fn clear_reference_in_inode: Referencia no encontrada en Inode.")
                }
            },
            None => panic!("fn clear_reference_in_inode: Intente desreferenciar un Inodo vacío.")
        }
    }

    /// Funcion que devuelve la referencia de memoria mutable de `Inode`.
    pub fn get_inode_as_mut(&mut self, ino: u64) -> Option<&mut Inode> {
        let index = (ino as usize) - 1;
        match &mut self.super_block[index] {
            Some(inode) => Some(inode),
            None => None
        }
    }

    /// Funcion que devuelve el `Inodo` especificado por su número `ino`.
    pub fn get_inode(&self, ino: u64) -> Option<&Inode> {
        let index = (ino as usize) - 1;
        match &self.super_block[index] {
            Some(inode) => Some(inode),
            None => None
        }
    }
    
    /// Funcion que busca el Inodo por nombre dentro de una matriz de referencias de Inodos principales.
    pub fn find_inode_in_references_by_name(&self, parent_inode_ino: u64, name: &str) -> Option<&Inode> {
        let index = (parent_inode_ino as usize) - 1;
        let parent_inode = &self.super_block[index];

        match parent_inode {
            Some(parent_inode) => {
                // Buscar la matriz de referencia de Inode
                for ino_ref in parent_inode.references.iter() {
                    // Si hay algún dato dentro de ino_ref, ingrese el bloque y obtenga ese contenido
                    if let Some(ino) = ino_ref {
                        let index: usize = (ino.clone() as usize) - 1;
                        let inode_ref = &self.super_block[index];

                        match inode_ref {
                            Some(inode) => {
                                let name_from_inode: String = inode.name.iter().collect::<String>();
                                let name_from_inode: &str = name_from_inode.as_str().trim_matches(char::from(0)); // Eliminación de caracteres '\0'
                                let name = name.trim();
                                println!("    - access(name={:?}, name_from_inode={:?}, equals={})", name, name_from_inode, name_from_inode == name);
                                
                                if name_from_inode == name {
                                    return Some(inode);
                                }
                            },
                            None => panic!("fn get_inode_by_name: referencia a inodo no encontrado!")
                        }
                    }
                }
            },
            None => panic!("fn get_inode_by_name: inodo padre no encontrado!")
        }

        return None;
    }

    /// Funcion que retorna un vector de refencia
    #[allow(dead_code)]
    pub fn get_references_from_inode(&self, ino: u64) -> &[Option<usize>; 128] {
        let index = (ino as usize) - 1;
        match &self.super_block[index] {
            Some(inode) => &inode.references,
            None => panic!("fn get_references_from_inode: inodo no encontrado!")
        }
    }

    /// Funcion que recupera el contenido de un bloque de memoria convertido a `str`
    #[allow(dead_code)]
    pub fn get_content(&self, block_index: usize) -> Option<&str> {
        let data = self.get_content_as_bytes(block_index);
        
        match &data {
            Some(data) => {
                Option::Some(str::from_utf8(&data).unwrap())
            },
            None => None
        }
    }

    /// Recupera una matriz de bytes prestados de un bloque especificado.
    ///
    /// # Ejemplos
    ///
    /// ```.
    /// let disk = disk::new(argumentos);
    /// let content: [u8] = disk.get_content_as_bytes(1);
    /// ```
    pub fn get_content_as_bytes(&self, block_index: usize) -> &Option<Box<[u8]>> {
        let memory_block = &self.memory_blocks[block_index];
        return &memory_block.data;
    }

    /// Escribir datos en bytes en un bloque de memoria
    ///
    /// # Ejemplos
    ///
    /// ```
    /// let content: Box<[u8]> = Box::from(content.as_bytes());
    /// let disk: disk = disk::new(argumentos);
    /// disk.write_content_as_bytes(1, contenido);
    /// ```
    ///
    /// Solo se escribe si es una ubicación de memoria válida
    pub fn write_content_as_bytes(&mut self, block_index: usize, content: Box<[u8]>) {
        if content.len() > self.block_size {
            panic!("No se puede guardar el contenido del archivo porque excede el tamaño del bloque de memoria {}", self.block_size);
        }

        let memory_block = MemoryBlock { data: Some(content) };
        self.memory_blocks[block_index] = memory_block;
    }

    /// Escribe una referencia al vector de referencia de un Inodo ino-numerado
    pub fn write_reference_in_inode(&mut self, ino: u64, ref_index: usize, ref_content: usize) {
        let index = (ino as usize) - 1;
        match &mut self.super_block[index] {
            Some(inode) => {
                inode.references[ref_index] = Some(ref_content);
            },
            None => panic!("fn write_reference_in_inode: inodo no encontrado!")
        }
    }

    pub fn write_to_disk(&mut self) {
        match serialize(&self.super_block) {
            Err(e) => {
                print!("¡Error al intentar escribir en el archivo inodes! {}", e);
                return;
            },
            Ok(v) => {
                let inode_file = format!("{}/inode.qrfs", &self.root_path);
                let mut inode_file = OpenOptions::new().write(true).open(inode_file).unwrap();
                match inode_file.write(&v) {
                    Err(e) => {
                        print!("¡Error al intentar escribir en el archivo inodes! {}", e);
                        return;
                    },
                    Ok(v) => v,
                };
            },
        };

        match serialize(&self.memory_blocks) {
            Err(e) => {
                print!("¡Error al intentar escribir en el archivo de disco! {}", e);
                return;
            },
            Ok(v) => {
                let disk_file = format!("{}/disco.qrfs", &self.root_path);
                let mut disk_file = OpenOptions::new().write(true).open(disk_file).unwrap();
                match disk_file.write(&v) {
                    Err(e) => {
                        print!("¡Error al intentar escribir en el archivo inodes! {}", e);
                        return;
                    },
                    Ok(v) => v,
                };
            },
        };
    }
}
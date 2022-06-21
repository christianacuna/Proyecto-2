extern crate fuse;
#[macro_use]
extern crate serde_big_array;
extern crate image;
mod persistencia;
mod serialization;

// Libreria para el manejo del file system en el user space
use fuse::{Filesystem, Request, ReplyCreate, ReplyEmpty, ReplyAttr, ReplyEntry, ReplyOpen, ReplyData, ReplyDirectory, ReplyWrite, FileType, FileAttr};
// Libreria para el manejo de banderas del codigo en C
use libc::{ENOSYS, ENOENT, EIO, EISDIR, ENOSPC};
// Liberia para obtener el tiempo
use time::{Timespec};
// Libreria para obtener el ambiente de ejecucion
use std::env;
// Libreria para el manejo de memoria
use std::mem;
// Libreria para obtener el mejor manejo de strings de nuestro sistema operativo
use std::ffi::OsStr;
// Libreria para la implementacion de nuestra persistencia
use crate::persistencia::{Disk, Inode};
// Libreria para verificar si un archivo existe
use std::path::Path;
use std::{
    fs::File,
    io::{BufWriter, Write, Read},
};
use std::fs;
use qrcode_generator::QrCodeEcc;
use image::{ImageFormat};

struct QrFS {
    disk: Disk
}

impl QrFS {
    /// Inicializa FS con el tamaño especificado en `memory_size` con bloques de memoria de tamaño    
    /// `block_size`.
    fn new(root_path: String) -> Self {
        let max_files: usize = 1024;
        let memory_size: usize = 1024 * 1024 * 1024;
        let block_size: usize = max_files * (mem::size_of::<Box<[Inode]>>() + mem::size_of::<Inode>());

        let disk = Disk::new(root_path, memory_size, block_size);

        QrFS {
            disk
        }
    }
}
#[allow(unused_must_use)]
impl Drop for QrFS {
    fn drop(&mut self) {
        println!("\nsaving content...");
        &self.disk.write_to_disk();
        println!("success!");
    }
}

/// Implementación de las funciones disponibles en la librería `rust-fuse`
impl Filesystem for QrFS {
    fn lookup(
        &mut self, 
        _req: &Request, 
        parent: u64, 
        name: &OsStr, 
        reply: ReplyEntry
    ) {
        println!("lookup(parent={:?}, name={:?})", parent, name);
        let file_name = name.to_str().unwrap();
        let inode = self.disk.find_inode_in_references_by_name(parent, file_name);

        match inode {
            Some(inode) => {
                let ttl = time::now().to_timespec();
                println!("        - lookup(parent={:?}, attr={:?})", parent, inode.attributes);
                reply.entry(&ttl, &inode.attributes, 0)
            },
            None => reply.error(ENOENT) // “No such file or directory.”
        }
    }

    fn create(
        &mut self, 
        _req: &Request, 
        parent: u64, 
        name: &OsStr, 
        mode: u32, 
        flags: u32, 
        reply: ReplyCreate
    ) {
        println!("create(name={:?}, mode={}, flags={})", name, mode, flags);

        let ref_index = self.disk.find_index_of_empty_reference_in_inode(parent);
        // Si no hay más espacio en el vector de referencias, indica que no es posible ubicar más archivos dentro de la carpeta        
        if ref_index == None {
            println!("¡No se pueden crear más archivos en este directorio!");
            reply.error(EIO); // “Input/output error.”
            return;
        }

        let ino_available = self.disk.find_ino_available();
        let memory_block_index = self.disk.find_index_of_empty_memory_block();

        if ino_available == None || memory_block_index == None {
            reply.error(ENOSPC); // “No hay mas espacio en el dispositivo.”
            return;
        }

        let ino_available = ino_available.unwrap();
        let memory_block_index = memory_block_index.unwrap();

        let ts = time::now().to_timespec();

        let attr = FileAttr {
            ino: ino_available,
            size: 0,
            blocks: 1,
            atime: ts,
            mtime: ts,
            ctime: ts,
            crtime: ts,
            kind: FileType::RegularFile,
            perm: 0o755,
            nlink: 0,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags,
        };
        
        let name = name.to_str().unwrap();
        let name: Vec<char> = name.chars().collect();

        let mut name_char = ['\0'; 64];
        name_char[..name.len()].clone_from_slice(&name);

        let mut inode = Inode {
            name: name_char,
            attributes: attr,
            references: [None; 128]
        };

        inode.references[0] = Some(memory_block_index);

        self.disk.write_inode(inode);
        let content: Box<[u8]> = Box::default();
        self.disk.write_content_as_bytes(memory_block_index, content);

        // Agregua la referencia del inodo creada en las referencias vectoriales al inodo "principal" (desde el directorio)
        let ref_index = ref_index.unwrap();
        self.disk.write_reference_in_inode(parent, ref_index, ino_available as usize);

        reply.created(&ts, &attr, 1, ino_available, flags)
    }

    fn fsync(
        &mut self, 
        _req: &Request, 
        ino: u64, 
        fh: u64, 
        datasync: bool, 
        reply: ReplyEmpty
    ) { 
        println!("fsync(ino={}, fh={}, datasync={})", ino, fh, datasync);
        reply.error(ENOSYS);
    }

    fn setattr(
        &mut self, 
        _req: &Request, 
        ino: u64, 
        _mode: Option<u32>, 
        uid: Option<u32>, 
        gid: Option<u32>, 
        size: Option<u64>, 
        atime: Option<Timespec>, 
        mtime: Option<Timespec>, 
        _fh: Option<u64>, 
        crtime: Option<Timespec>, 
        _chgtime: Option<Timespec>, 
        _bkuptime: Option<Timespec>, 
        flags: Option<u32>, 
        reply: ReplyAttr
    ) {
        println!("setattr(ino={})", ino);
        let inode = self.disk.get_inode_as_mut(ino);
        
        match inode {
            Some(inode) => {
                if let Some(size) = size { inode.attributes.size = size; }
                if let Some(atime) = atime { inode.attributes.atime = atime; }
                if let Some(mtime) = mtime { inode.attributes.mtime = mtime; }
                if let Some(crtime) = crtime { inode.attributes.crtime = crtime; }
                if let Some(gid) = gid { inode.attributes.gid = gid; }
                if let Some(uid) = uid { inode.attributes.uid = uid; }
                if let Some(flags) = flags { inode.attributes.flags = flags; }

                let ttl = time::now().to_timespec();

                reply.attr(&ttl, &inode.attributes)
            },
            None => reply.error(ENOENT)
        }
    }

    fn getattr(
        &mut self,
        _req: &Request,
        ino: u64,
        reply: ReplyAttr
    ) {
        println!("getattr(ino={})", ino);

        match self.disk.get_inode(ino) {
            Some(inode) => {
                let ttl = time::now().to_timespec();
                reply.attr(&ttl, &inode.attributes);
            },
            None => reply.error(ENOENT)
        }
    }

    fn mkdir(
        &mut self, 
        _req: &Request, 
        parent: u64, 
        name: &OsStr, 
        _mode: u32, 
        reply: ReplyEntry
    ) {
        let reference_index = self.disk.find_index_of_empty_reference_in_inode(parent);
        
        match reference_index {
            Some(reference_index) => {

                let ino = self.disk.find_ino_available();
                match ino {
                    Some(ino) => {
                        let ts = time::now().to_timespec();
                        let attr = FileAttr {
                            ino: ino as u64,
                            size: 0,
                            blocks: 1,
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

                        let name = name.to_str().unwrap();
                        let name: Vec<char> = name.chars().collect();

                        let mut name_char = ['\0'; 64];
                        name_char[..name.len()].clone_from_slice(&name);

                        let inode = Inode {
                            name: name_char,
                            attributes: attr,
                            references: [None; 128]
                        };

                        self.disk.write_inode(inode);
                        self.disk.write_reference_in_inode(parent, reference_index, ino as usize);

                        reply.entry(&ts, &attr, 0);
                    },
                    None => reply.error(ENOSPC) // “No space left on device.”
                }
            },
            None => { println!("¡Límite de archivos dentro de la carpeta alcanzado!"); reply.error(EIO); }
        }
    }

    fn rmdir(
        &mut self, 
        _req: &Request, 
        parent: u64, 
        name: &OsStr, 
        reply: ReplyEmpty
    ) {
        let name = name.to_str().unwrap();
        let inode = self.disk.find_inode_in_references_by_name(parent, name);

        match inode {
            Some(inode) => {
                let ino = inode.attributes.ino;
                self.disk.clear_reference_in_inode(parent, ino as usize);
                self.disk.clear_inode(ino);

                reply.ok();
            },
            None => reply.error(EIO) // "Input/output error."
        }
    }

    fn open(
        &mut self,
        _req: &Request,
        ino: u64,
        flags: u32,
        reply: ReplyOpen
    ) {
        println!("open(ino={}, flags={})", ino, flags);

        let inode = self.disk.get_inode(ino);

        match inode {
            Some(_) => reply.opened(ino, flags),
            None => reply.error(ENOSYS)
        }
    }

    fn read(
        &mut self, 
        _req: &Request, 
        ino: u64, 
        fh: u64, 
        offset: i64, 
        size: u32, 
        reply: ReplyData
    ) {
        println!("read(ino={}, fh={}, offset={}, size={})", ino, fh, offset, size);

        let memory_block = self.disk.get_content_as_bytes(ino as usize);
        
        match memory_block {
            Some(memory_block) => reply.data(memory_block),
            None => reply.error(EIO)
        }
    }

    fn readdir(
        &mut self, 
        _req: &Request, 
        ino: u64, 
        fh: u64, 
        offset: i64, 
        mut reply: ReplyDirectory
    ) {
        println!("readdir(ino={}, fh={}, offset={})", ino, fh, offset);

        // Pequeño "ajuste técnico" para mostrar el "." y ".." en la primera carpeta.
        if ino == 1 {
            if offset == 0 {
                reply.add(1, 0, FileType::Directory, ".");
                reply.add(1, 1, FileType::Directory, "..");
            }
        }

        // Inodo "padre" (el directorio)
        let inode: Option<&Inode> = self.disk.get_inode(ino);

        // Desplazamiento representado como el tamaño entero del Inodo, ya que todo el contenido se leerá a la vez
        // del directorio. Si el desplazamiento es el mismo que el tamaño del inodo principal, haga clic en "Aceptar" y devuelva el contenido.
        if mem::size_of_val(&inode) == offset as usize {
            reply.ok();
            return;
        }

        match inode {
            Some(inode) => {
                let references = inode.references;
                // Ciclo a través del vector de referencia del inodo padre. Cada posición indica un archivo que está presente
                // en el directorio.
                for ino in references.iter() {

                    if let Some(ino) = ino {
                        let inode = self.disk.get_inode(*ino as u64);

                        if let Some(inode_data) = inode {
                            if inode_data.attributes.ino == 1 {
                                continue;
                            }

                            let name = inode_data.name.iter().collect::<String>();
                            println!("    - readdir(ino={}, name={})", inode_data.attributes.ino, name);
                            let offset = mem::size_of_val(&inode) as i64;
                            reply.add(inode_data.attributes.ino, offset, inode_data.attributes.kind, name);
                        }
                    }
                }

                reply.ok()
            },
            None => { println!("ERROR ino={:?}", ino); reply.error(ENOENT) }
        }
    }

    fn write(
        &mut self, 
        _req: &Request, 
        ino: u64, 
        _fh: u64, 
        offset: i64, 
        data: &[u8], 
        _flags: u32, 
        reply: ReplyWrite
    ) {
        println!("write(ino={}, offset={}, data={})", ino, offset, data.len());
        let inode = self.disk.get_inode_as_mut(ino);
        let content: Box<[u8]> = data.to_vec().into_boxed_slice();

        match inode {
            Some(inode) => {
                inode.attributes.size = data.len() as u64;
                let index = (ino as usize) - 1;
                self.disk.write_content_as_bytes(index, content);
                reply.written(data.len() as u32);
            },
            None => {
                println!("Nodo no encontrado");
                reply.error(ENOENT);
            }
        }
    }

    fn unlink(
        &mut self, 
        _req: &Request, 
        parent: u64, 
        name: &OsStr, 
        reply: ReplyEmpty
    ) {
        let name = name.to_str().unwrap();
        let inode = self.disk.find_inode_in_references_by_name(parent, name);

        match inode {
            Some(inode) => {
                if inode.attributes.kind == FileType::Directory {
                    reply.error(EISDIR);
                } else {
                    let ino = inode.attributes.ino;
                    let memory_block_index = (ino as usize) - 1;
                    self.disk.clear_inode(ino);
                    self.disk.clear_memory_block(memory_block_index);
                    self.disk.clear_reference_in_inode(parent, ino as usize);
                    reply.ok()
                }
            },
            None => reply.error(EIO)
        }
    }
}
fn get_file_as_byte_vec(filename: &String) -> Vec<u8> {
    let mut f = File::open(&filename).expect("no file found");
    let metadata = fs::metadata(&filename).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");

    buffer
}
#[allow(unused_must_use)]
fn main() {
    let mountpoint = match env::args().nth(1) {
        Some(path) => path,
        None => {
            println!("Usage: {} <MOUNTPOINT>", env::args().nth(0).unwrap());
            return;
        }
    };
    let disk_file_path = format!("{}/disco.qrfs",  mountpoint);
    let inode_table_file_path = format!("{}/inode.qrfs",  mountpoint);
    let document_file_path = format!("{}/disco.txt",  mountpoint);
    let qrcode_file_path = format!("{}/disco.png",mountpoint);
    let qrcode_file_path2 = format!("{}/inode.png",mountpoint);
    if !(Path::new(&disk_file_path).exists()){
        println!("No se encuentra el disco del filesystem")
    }
    else if !(Path::new(&inode_table_file_path).exists()){
        println!("No se encuentra el i-node del filesytem")
        
    } else{
        let l = QrFS::new(mountpoint.clone());
        let write_file = File::create(document_file_path.clone()).unwrap();
        let mut writer = BufWriter::new(&write_file);

        println!("QrFS Valido!");
        println!("Creando el archivo imprimible");
        write!(&mut writer, "{:?}", l.disk);
        let contents = get_file_as_byte_vec(&disk_file_path);
        let contents2 = get_file_as_byte_vec(&inode_table_file_path);
        let result: Vec<u8> = qrcode_generator::to_png_to_vec(contents, QrCodeEcc::Low, 1024).unwrap();
        let result2: Vec<u8> = qrcode_generator::to_png_to_vec(contents2, QrCodeEcc::Low, 1024).unwrap();
        //println!("{:?}", result);
        match image::load_from_memory_with_format(&result, ImageFormat::Png) {
            Ok(_img) => {
                println!("Creando el Codigo QR para el Disco");
                std::fs::write(qrcode_file_path, result).unwrap();
            }
            Err(_) => {
                println!("input is not png");
            }
        }
        match image::load_from_memory_with_format(&result2, ImageFormat::Png) {
            Ok(_img) => {
                println!("Creando el Codigo QR para la tabla I-Node");
                std::fs::write(qrcode_file_path2, result2).unwrap();
            }
            Err(_) => {
                println!("input is not png");
            }
        }
    }
    
}
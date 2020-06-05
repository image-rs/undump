use std::io;
use std::cell::RefCell;
use std::collections::HashMap;

use tokio::fs;
use tokio::io::AsyncReadExt;

fn main() {
    let mut args: Vec<_> = std::env::args().skip(1).collect();

    let path = args.pop().unwrap_or_else(|| {
        println!("Please provide a target directory to recurse");
        std::process::exit(1);
    });

    let mode = Mode::from_args(path, args);
    match mode.run() {
        Ok(()) => {},
        Err(fatal) => eprintln!("Fatal error: {:?}", fatal),
    }
}

async fn process_all(files: &mut dyn Iterator<Item=std::path::PathBuf>) {
    let count: HashMap<image::ImageFormat, usize> = HashMap::new();
    let count = RefCell::new(count);
    let stream = RefCell::new(files);

    let workers: Vec<_> = (0..32)
        .map(|_| read_files(&stream, &count))
        .collect();

    let _ = futures::future::join_all(workers).await;
    println!("Statistics {:?}", count);
}

async fn read_files(
    supplier: &RefCell<&mut dyn Iterator<Item=std::path::PathBuf>>,
    count: &RefCell<HashMap<image::ImageFormat, usize>>,
) {
    loop {
        let next = supplier.borrow_mut().next();
        if let Some(path) = next {
            for_file(path, count).await;
        } else {
            break;
        }
    }
}

async fn for_file(path: std::path::PathBuf, count: &RefCell<HashMap<image::ImageFormat, usize>>) {
    let mut file = match fs::File::open(path).await {
        Ok(file) => file,
        Err(_) => return,
    };

    let mut buffer: Vec<u8> = vec![0; 512];
    let _ = file.read_exact(&mut buffer).await;

    let reader = io::Cursor::new(buffer.as_slice());
    if let Ok(read) = image::io::Reader::new(reader).with_guessed_format() {
        if let Some(format) = read.format() {
            *count.borrow_mut().entry(format).or_default() += 1usize;
        }
    }
}

enum Mode {
    Dir(String),
    Recurse(String),
}

impl Mode {
    fn from_args(path: String, args: Vec<String>) -> Self {
        match &*args {
            [] => Mode::Dir(path),
            [arg] if arg == "--recurse" || arg == "-r" => Mode::Recurse(path),
            _ => {
                eprintln!("Call as: undump [-r] dir");
                std::process::exit(1);
            }
        }
    }

    fn run(self) -> io::Result<()> {
        match self {
            Self::Dir(path) => {
                let mut files = std::fs::read_dir(path)?
                    .filter_map(Result::ok)
                    .map(|entry| entry.path());
                Self::spawn(process_all(&mut files));
            }
            Self::Recurse(path) => {
                let pattern = format!("{}/**/*", path);
                let mut files = glob::glob(&pattern)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?
                    .filter_map(Result::ok);
                Self::spawn(process_all(&mut files));
            },
        }

        Ok(())
    }

    fn spawn(fut: impl std::future::Future<>) {
        use tokio::runtime::Runtime;
        use tokio::task;

        let mut rt = Runtime::new().unwrap();
        let local = task::LocalSet::new();
        local.block_on(&mut rt, fut);
    }
}

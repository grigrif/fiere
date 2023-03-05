use clap::{arg, Parser};
use colored;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::multipart::Part;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use std::{env, fs};

/// Search for a pattern in a file and display the lines that contain it.
#[derive(Parser)]
struct CliUpload {
    pattern: String,
    path: std::path::PathBuf,
    password: Option<String>,
    max_download: Option<u64>,
    expiry: Option<String>,
}

#[derive(Parser)]
struct CliDownload {
    pattern: String,
    identifier: String,
    path: Option<String>,
    password: Option<String>,
}

#[derive(Parser)]
struct CliDelete {
    pattern: String,
    identifier: String,
}

#[derive(Deserialize)]
struct PostFile {
    secret_key: String,
}

#[derive(Deserialize)]
struct Submit {
    identifier: String,
    expired_at: i64,
}

#[derive(Serialize, Deserialize, Clone)]
struct FilePart {
    file_size: i64,
    hash: String,
    identifier: String,
    offset: i64,
}

#[derive(Serialize, Deserialize, Clone)]
struct MiniFile {
    expired_at: i64,
    file_size: i64,
    name: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct FileInfo {
    file: MiniFile,
    parts: Vec<FilePart>,
}
#[derive(Serialize, Deserialize)]
struct Upload {
    secret_key: String,
    path: String,
    file_offset: u64,
    curr_offset: i64,
}
#[derive(Serialize, Deserialize)]
struct Download {
    identifier: String,
    file: FileInfo,
    path: String,
    curr_offset: i64,
}
#[derive(Serialize, Deserialize)]
struct Config {
    secrets: HashMap<String, String>,
    uploads: HashMap<String, Upload>,
    downloads: HashMap<String, Download>,
}

fn load_config() -> Config {
    if let Ok(file) = File::open("./config.json") {
        let reader = BufReader::new(file);
        let config: Config = serde_json::from_reader(reader).unwrap();
        return config;
    } else {
        return Config {
            secrets: HashMap::new(),
            uploads: HashMap::new(),
            downloads: HashMap::new(),
        };
    }
}

fn save_config(config: &Config) {
    if let Ok(file) = File::create("./config.json") {
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, config).unwrap();
        return;
    }
    panic!("cannot save file");
}

fn get_expiry(str: String) -> i32 {
    let mut curr = 0;
    let mut to_add = 0;
    for c in str.chars() {
        if c.is_numeric() {
            curr = curr * 10 + c.to_string().parse::<u64>().expect("");
        } else {
            match c.to_string().as_str() {
                "s" => to_add += curr,
                "m" => to_add += curr * 60,
                "h" => to_add += curr * 60 * 60,
                "d" => to_add += curr * 60 * 60 * 24,
                "M" => to_add += curr * 60 * 60 * 24 * 31,
                _ => {}
            }
            curr = 0;
        }
    }
    to_add += SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System time before epoche")
        .as_secs();
    println!("{}", to_add);
    return to_add as i32;
}

fn upload(params: CliUpload) {
    let mut config = load_config();

    let mut is_load = false;
    let mut secret_key = "".to_string();
    let client = reqwest::blocking::Client::new();
    if let Some(upload) = config.uploads.get(params.path.to_str().unwrap()) {
        secret_key = upload.secret_key.clone();
        let resp = client
            .get(format!("http://localhost:8080/status/{}", secret_key))
            .send()
            .expect("cound not send request");
        if resp.status() == reqwest::StatusCode::OK {
            println!("{}", "Resuming Download".green());
            is_load = true;
        }
    }
    if !is_load {
        let resp = client.post("http://localhost:8080/file").send().unwrap();
        let post_file = resp.json::<PostFile>().unwrap();
        secret_key = post_file.secret_key.clone();
    }
    let mut file = File::open(&params.path).expect("no file");
    let bar = ProgressBar::new(*&file.metadata().unwrap().len());
    let mut buffer = [0; 500];
    let mut bytes: Vec<u8> = vec![];
    let mut offset = 0;
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .expect("template fail"),
    );
    if is_load {
        let upload = config.uploads.get(params.path.to_str().unwrap()).unwrap();
        offset = upload.curr_offset;
        bar.inc(upload.file_offset as u64);
        file.seek(SeekFrom::Start(upload.file_offset as u64))
            .unwrap();
    }

    loop {
        match file.read(buffer.as_mut()) {
            Ok(n) => {
                bar.inc(n as u64);

                if n == 0 && bytes.is_empty() {
                    break;
                }
                bytes.extend_from_slice(&buffer[0..n]);
                if &bytes.len() < &(4 * 1_000_000) && n != 0 {
                    continue;
                }

                offset += 1;
                let hash = format!("{:x}", md5::compute(&bytes));
                let secret_key = secret_key.clone();

                let form = reqwest::blocking::multipart::Form::new()
                    .text("secret_key", secret_key.clone())
                    .text("offset", offset.to_string())
                    .text("hash", hash)
                    .part(
                        "file",
                        Part::reader_with_length(Cursor::new(bytes.clone()), bytes.len() as u64),
                    );

                let resp = client
                    .post("http://localhost:8080/part_file")
                    .multipart(form)
                    .send();
                bytes.clear();
                let pos = file.stream_position().unwrap();
                config.uploads.insert(
                    params.path.to_str().unwrap().to_string(),
                    Upload {
                        secret_key: secret_key.clone(),
                        path: params.path.to_str().unwrap().to_string(),
                        file_offset: pos,
                        curr_offset: offset,
                    },
                );
                save_config(&config);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Reached the end of the file
                break;
            }
            Err(_e) => {
                // Handle other errors
                panic!("not found");
            }
        }
    }
    let max_download = params.max_download.unwrap_or(10_000).to_string();
    let expiry = get_expiry(params.expiry.unwrap_or("7d".to_string())).to_string();
    let filename = params
        .path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let resp = client
        .post(format!("http://localhost:8080/file/{}", secret_key))
        .query(&[
            ("expire", expiry),
            ("name", filename),
            ("max_download", max_download),
        ])
        .send()
        .unwrap();
    let submit = resp.json::<Submit>().unwrap();
    bar.finish_and_clear();
    let identifier = submit.identifier.clone();
    let mut config = load_config();
    config.secrets.insert(identifier, secret_key);
    save_config(&config);
    println!("{}", submit.expired_at);
    println!(
        "Upload is finish. Id: {}",
        submit.identifier.as_str().green()
    );
}

fn download(params: CliDownload) -> Option<i32> {
    let mut config = load_config();
    let resp;
    if let Some(last) = config.downloads.get(&params.identifier) {
        resp = last.file.clone();
    } else {
        let resp_ = reqwest::blocking::get(format!(
            "http://localhost:8080/file_info/{}",
            params.identifier
        ))
        .expect("coundn't send request");
        if resp_.status() == 404 {
            println!("{}", "File Identifier not found".red());
            return Some(1);
        }
        resp = resp_.json::<FileInfo>().expect("couldn't parse response");
    
    }

    println!("Will download: {}", &resp.file.name);
    let path = params.path;
    let bar = ProgressBar::new(resp.file.file_size as u64);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .expect("template fail"),
    );

    if &path.is_some() == &true {
        fs::create_dir_all(&path.clone().expect("coundn't create directory"))
            .expect("coundn't create directory");
    }

    let path = match path {
        Some(path) => format!("./{}/{}", path, resp.file.name),
        None => {
            format!("./{}", resp.file.name)
        }
    };
    let mut n = 0;
    let mut file;
    let mut i = 0;

    if let Some(last) = config.downloads.get(&params.identifier) {
        file = OpenOptions::new().write(true).append(true).open(path.as_str()).unwrap();
        bar.inc(file.metadata().unwrap().len() );
        i = last.curr_offset;
        n = last.curr_offset;
        file.seek(SeekFrom::End((0)));
    } else {
        file = File::create(&path).expect("coundn't create file");
    }
    for part in &resp.parts {
        if n > 0 {
            n  -= 1;
            continue;
        }
        let bytes =
            reqwest::blocking::get(format!("http://localhost:8080/file/{}", &part.identifier))
                .ok()?
                .bytes()
                .ok()?;
        file.write_all(&*bytes);
        bar.inc(part.file_size as u64);
        config.downloads.insert(
            params.identifier.clone(),
            Download {
                identifier: "".to_string(),
                file: resp.clone(),
                path: path.clone(),
                curr_offset: i,
            },
        );
        save_config(&config);
        i += 1;
    }
    config.downloads.remove(&params.identifier.clone());
    save_config(&config);

    bar.finish_and_clear();
    println!("finished");
    return Some(1);
}

fn delete(params: CliDelete) {
    let client = reqwest::blocking::Client::new();
    let mut config = load_config();
    if let Some(secret) = config.secrets.get(params.identifier.as_str()) {
        let resp = client
            .delete(format!("http://localhost:8080/file/{}", secret))
            .send()
            .expect("couldn't reach server");
        if resp.status() == 404 {
            println!("{}", "File Identifier not found".red());
        } else {
            println!("{}", "File deleted".green());
        }
        config.secrets.remove(params.identifier.as_str());
        save_config(&config);
    } else {
        println!("{}", "File Identifier not found".red());
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    //upload(CliUpload {pattern: "upload".to_string(), path: PathBuf::from("test.txt"), password: None, max_download: None, expiry: None });
    //delete(CliDelete {identifier: "eb890971".to_string(), pattern: "azd".to_string()});
    download(CliDownload {
        pattern: "a91e8f49".to_string(),
        identifier: "f11debf4".to_string(),
        path: None,
        password: None,
    });
    match args.get(0).unwrap().as_str() {
        "upload" => upload(CliUpload::parse()),
        "download" => {download(CliDownload::parse());},
        "delete" => delete(CliDelete::parse()),
        _ => {
            println!("Sub command not found [upload, delete, download]");
        }
    }
}

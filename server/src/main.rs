use std::{fs, io};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use actix_multipart::Multipart;
use actix_multipart::MultipartError::Payload;
use actix_web::{App, HttpResponse, HttpServer, middleware, Responder, web, get, post, delete, error, Error};
use sqlx::{Connection, ConnectOptions, Pool, Sqlite, SqlitePool};
use uuid::Uuid;
use serde;
use serde_json::json;
use futures_util::StreamExt as _;
mod payload;
use actix_files::NamedFile;
use actix_rt::time;
use sqlx::sqlite::SqliteConnectOptions;

async fn periodic_delete(db: &Pool<Sqlite>) {
    let time_span = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("System time before epoche").as_secs().to_string();
    let results = sqlx::query_file!("./sql/suppress_file_part_expired.sql", time_span, time_span)
        .fetch_all(db)
        .await.unwrap();

    for i in results {
        let identifier: String = i.identifier.unwrap();
        let info = fs::remove_file(format!("./data/{}", identifier));
        if info.is_err() {
            print!("{:?}", info.err().unwrap());
        }
    }



}

#[post("/file")]
async fn prepare_file( db: web::Data<Pool<Sqlite>>) -> impl Responder {
    let secret_key = Uuid::new_v4().to_string();
    let identifier = &Uuid::new_v4().to_string()[0..8];
    let time_span = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("System time before epoche").as_secs() + 3600 * 24;
   let result = sqlx::query(include_str!("../sql/prepare.sql")).
       bind(&time_span.to_string()).bind(&secret_key).bind(identifier).execute(db.get_ref()).await;
    if result.is_err() {
        print!("{:?}", result.err().unwrap());
        return HttpResponse::InternalServerError().finish();
    }
    return HttpResponse::Ok().json(json!({
        "secret_key": secret_key,
        "expired_at": time_span
    }));
}

#[get("/file/{identifier}")]
async fn get_file_identifier(path: web::Path<String>) -> io::Result<NamedFile> {
    let identifier = path.into_inner();
    Ok(NamedFile::open(format!("./data/{}", identifier))?)
}

#[get("/file_info/{identifier}")]
async fn get_file(path: web::Path<(String)>, db: web::Data<Pool<Sqlite>>, ) -> Result<HttpResponse, Error>  {
    let identifier = path.into_inner();
    let parts = sqlx::query_file_as!(payload::GetPartsResponse, "./sql/get_filepart.sql", identifier).fetch_all(db.get_ref()) ;
    let response = sqlx::query_file_as!(payload::GetOneFileResponse, "./sql/get_file.sql", identifier).fetch_all(db.get_ref())
        .await.map_err(|e| error::ErrorNotFound("e"))?;
    if response.len()  == 0 {
        return Ok(HttpResponse::NotFound().finish());
    }

    let parts = parts.await.map_err(|e| error::ErrorNotFound("e"))?;

return  Ok(HttpResponse::Ok().json(json!({
    "file": response,
    "parts":parts
})))
}





#[post("/file/{secret_key}")]
async fn submit(path: web::Path<(String)>, submit_payload: web::Query<payload::SubmitPayload>, db: web::Data<Pool<Sqlite>>) -> Result<HttpResponse, Error> {
    let secret_key = path.into_inner();
    let max_download = submit_payload.max_download.unwrap_or(10_000).to_string();
    let expire = submit_payload.expire;
    let name = &submit_payload.name;
    let response = sqlx::query_file_as!(payload::SubmitResponse, "./sql/submit.sql",
       max_download , expire, name, secret_key
).fetch_all(db.get_ref())
        .await.map_err(|e| error::ErrorNotFound("e"))?;
    if &response.len() == &0 {
        return Ok(HttpResponse::NotFound().finish());
    }
    return Ok(HttpResponse::Ok().json(&response[0]))
}


#[delete("/file/{secret_key}")]
async fn delete(path: web::Path<String>, db: web::Data<Pool<Sqlite>>)-> Result<HttpResponse, Error> {
    let secret_key = path.into_inner();
    let results = sqlx::query!("DELETE FROM FilePart WHERE file_id = (SELECT file_id FROM File WHERE secret_key = ?) RETURNING identifier", secret_key)
        .fetch_all(db.get_ref()) // -> Vec<{ country: String, count: i64 }>
        .await.map_err(|e| error::ErrorBadRequest("e"))?;
    for i in results {
        let identifier: String = i.identifier.unwrap();
        let info = fs::remove_file(format!("./data/{}", identifier));
        if info.is_err() {
            print!("{:?}", info.err().unwrap());
        }
    }
    let results = sqlx::query!("DELETE FROM File WHERE secret_key = ? RETURNING file_id", secret_key).fetch_all(db.get_ref()) // -> Vec<{ country: String, count: i64 }>
        .await.map_err(|e| error::ErrorBadRequest("e"))?;

    if results.len() == 0 {
        return Ok(HttpResponse::NotFound().finish());
    }

    return Ok(HttpResponse::Ok().json(json!({
        "message": "ok"
    })));
}

#[post("/part_file")]
async fn part_file(mut payload: Multipart, db: web::Data<Pool<Sqlite>>) -> Result<HttpResponse, Error>  {
    let mut offset = None;
    let mut file: Option<Vec<u8>> = None;
    let mut secret_key: Option<String>  = None;
    let mut hash: Option<String> = None;

    while let Some(item) = payload.next().await {
        let mut field = item?;
        let mut bytes = vec![];
        while let Some(chunk) = field.next().await {
            bytes.extend(chunk.unwrap());
            if bytes.len() > 4 * 1000_000 {
                return Ok(HttpResponse::PayloadTooLarge().finish());
            }
        }


        match field.name() {
            "offset" => {
                offset = Some(str::parse::<i32>(std::str::from_utf8(&bytes)
                    .map_err(|e| error::ErrorBadRequest("e"))?).map_err(|e| error::ErrorBadRequest("e"))?);
            }
            "hash" => {
                hash = Some(std::str::from_utf8(&bytes).map_err(|e| error::ErrorBadRequest("e"))?.to_string());
            }
            "file" => {
                file = Some(bytes);
            }
            "secret_key" => {
                secret_key = Some(std::str::from_utf8(&bytes).map_err(|e| error::ErrorBadRequest("e"))?.to_string());
            }
            _ => {}
        }

    }
    if file.is_some() && hash.is_some() && secret_key.is_some() && offset.is_some() {
        let digest = md5::compute(file.as_ref().unwrap());
        if &format!("{:x}", digest) != hash.as_ref().unwrap() {
            return Ok(HttpResponse::BadRequest().body("invalid hash"))
        }

        let identifier = &Uuid::new_v4().to_string()[0..8];

        let result = sqlx::query(include_str!("../sql/insert_filepart.sql"))
            .bind(&secret_key.as_ref().unwrap())
            .bind(&file.as_ref().unwrap().len().to_string())
            .bind(&hash.as_ref().unwrap())
            .bind(&identifier)
            .bind(offset.unwrap().to_string()).execute(db.get_ref()).await;

        if result.is_err() {
            print!("{:?}", result.err().unwrap());
            return Ok(HttpResponse::InternalServerError().finish());
        }
        let mut file_ = File::create(format!("data/{}", identifier))?;
        file_.write_all(file.as_ref().unwrap())?;

        return Ok(HttpResponse::Ok().json(json!({
            "message": "all good"
        })));
    }
    Ok(HttpResponse::BadRequest().body("Not all params furnish"))
}



#[get("/status/{secret_key}")]
async fn status(path: web::Path<String>, db: web::Data<Pool<Sqlite>>) -> Result<HttpResponse, Error> {
    let secret_key = path.into_inner();
    let status = sqlx::query_file_as!(payload::Status, "./sql/status.sql", secret_key).fetch_all(db.get_ref())
        .await.map_err(|e| error::ErrorNotFound("e"))?;
    if status.len() == 0 {
        return Err(error::ErrorNotFound("e"));
    }
    Ok(HttpResponse::Ok().json(status))
}

#[actix_web::main]
 async fn main() -> std::io::Result<()> {
//server.db
    let pool = Arc::new(SqlitePool::connect(":memory:").await.expect("couldn't connect"));

    sqlx::query(include_str!("../sql/migrate.sql")).execute(pool.clone().as_ref()).await.unwrap();


    fs::create_dir("data");
    let pool_ = Arc::clone(&pool);
    actix_rt::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(20));
        let pool_ = Arc::clone(&pool_);
        loop {
            interval.tick().await;
            let result = periodic_delete(&pool_.as_ref().clone()).await;
        }
    });


    // start HTTP server
    HttpServer::new(move || {
        App::new()
            // store db pool as Data object
            .app_data(web::Data::new(pool.as_ref().to_owned()))
            .service(get_file)
            .service(submit)
            .service(delete)
            .service(prepare_file)
            .service(part_file)
            .service(status)
            .service(get_file_identifier)
            .wrap(middleware::Logger::default())
    }).bind(("127.0.0.1", 8080))?.workers(2).run().await
}

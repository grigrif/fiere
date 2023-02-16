create table if Not exists File  (
    file_id   INTEGER PRIMARY KEY ,
    file_size INT NOT NULL DEFAULT 0,
    expired_at TIMESTAMP NOT NULL,
    secret_key TEXT NOT NULL UNIQUE,
    submitted BOOLEAN NOT NULL DEFAULT 0,
    identifier Text NOT NULL UNIQUE,
    download int not null default 0,
    max_download int not null default 10000,
    name Text NOT NULL DEFAULT ""
);
CREATE TABLE if Not exists FilePart (
    part_id INTEGER PRIMARY KEY ,
    file_id INT NOT NULL,
    file_size INT NOT NULL,
    identifier TEXT NOT NULL UNIQUE,
    hash TEXT not null,
    offset INT NOT NULL,
    FOREIGN KEY (file_id) REFERENCES File(file_id)
);

DELETE  from FilePart WHERE file_id = (SELECT file_id from File WHERE expired_at < ? and FilePart.file_id = File.file_id) RETURNING identifier;
DELETE  From File WHERE expired_at < ?;
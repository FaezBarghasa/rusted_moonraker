use surrealdb::engine::local::Db;
use surrealdb::Surreal;

pub async fn run_migrations(db: &Surreal<Db>) -> Result<(), surrealdb::Error> {
    db.query("DEFINE TABLE gcode_files SCHEMAFULL;").await?;
    db.query("DEFINE FIELD file_path ON TABLE gcode_files TYPE string;").await?;
    db.query("DEFINE FIELD estimated_time ON TABLE gcode_files TYPE option<float>;").await?;
    db.query("DEFINE FIELD layer_height ON TABLE gcode_files TYPE option<float>;").await?;
    db.query("DEFINE FIELD slicer_type ON TABLE gcode_files TYPE option<string>;").await?;
    db.query("DEFINE FIELD thumbnail_path ON TABLE gcode_files TYPE option<string>;").await?;
    db.query("DEFINE INDEX file_path_idx ON TABLE gcode_files COLUMNS file_path UNIQUE;").await?;

    db.query("DEFINE TABLE print_history SCHEMAFULL;").await?;
    db.query("DEFINE FIELD file_path ON TABLE print_history TYPE string;").await?;
    db.query("DEFINE FIELD status ON TABLE print_history TYPE string;").await?; // "success", "failed"
    db.query("DEFINE FIELD print_time ON TABLE print_history TYPE float;").await?;
    db.query("DEFINE FIELD timestamp ON TABLE print_history TYPE int;").await?; // epoch timestamp

    db.query("DEFINE TABLE print_records SCHEMAFULL;").await?;
    db.query("DEFINE FIELD file_path ON TABLE print_records TYPE string;").await?;
    db.query("DEFINE FIELD status ON TABLE print_records TYPE string;").await?;
    db.query("DEFINE FIELD print_time ON TABLE print_records TYPE float;").await?;
    db.query("DEFINE FIELD timestamp ON TABLE print_records TYPE int;").await?;

    db.query("DEFINE TABLE web_layout_presets SCHEMAFULL;").await?;
    db.query("DEFINE FIELD name ON TABLE web_layout_presets TYPE string;").await?;
    db.query("DEFINE FIELD layout_data ON TABLE web_layout_presets TYPE string;").await?;
    db.query("DEFINE FIELD timestamp ON TABLE web_layout_presets TYPE int;").await?;
    db.query("DEFINE INDEX name_idx ON TABLE web_layout_presets COLUMNS name UNIQUE;").await?;

    Ok(())
}


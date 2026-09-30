fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (file, output) in [("wire.ts", postal_lib::wire_types()), ("wire.fixture.ts", postal_lib::wire_fixture())] {
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/utils")).join(file);
        if std::env::args().any(|arg| arg == "--check") {
            if !std::fs::read_to_string(path)?.lines().eq(output.lines()) {
                return Err("wire types changed; run pnpm generate:wire".into());
            }
        } else if std::env::args().any(|arg| arg == "--write") {
            std::fs::write(path, output)?;
        } else {
            print!("{output}");
        }
    }
    Ok(())
}

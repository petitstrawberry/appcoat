use appcoat_runtime::Resources;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Explicit deployment contract until Scarlet's launcher supplies a resource root.
    let root = std::env::var_os("APPCOAT_RESOURCE_ROOT")
        .unwrap_or_else(|| "/applications/scarlet-reader.app/resources".into());
    let resources = Resources::new(root)?;
    print!("{}", String::from_utf8(resources.read("greeting.txt")?)?);
    Ok(())
}

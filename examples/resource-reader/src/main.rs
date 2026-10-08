use appcoat_runtime::Resources;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resources = Resources::discover("dev.appcoat.resource-reader")?;
    print!("{}", String::from_utf8(resources.read("greeting.txt")?)?);
    println!("resource-root={}", resources.root().display());
    println!("cwd={}", std::env::current_dir()?.display());
    for argument in std::env::args().skip(1) {
        println!("arg={argument}");
    }
    Ok(())
}

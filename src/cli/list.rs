use crate::core::db::ops::ZlDatabase;
use crate::error::ZlResult;

pub fn handle(db: &ZlDatabase) -> ZlResult<()> {
    let packages = db.list_packages()?;

    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    println!(
        "{:<30} {:<20} {:<15} {:>8}",
        "Name", "Version", "Source", "Files"
    );
    println!("{}", "-".repeat(75));

    for pkg in &packages {
        println!(
            "{:<30} {:<20} {:<15} {:>8}",
            pkg.id.name,
            pkg.id.version,
            pkg.id.source,
            pkg.installed_files.len()
        );
    }

    println!("\n{} package(s) installed.", packages.len());
    Ok(())
}

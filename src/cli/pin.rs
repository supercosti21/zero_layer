use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};

use super::{PinArgs, UnpinArgs};

pub fn handle_pin(args: PinArgs, db: &ZlDatabase) -> ZlResult<()> {
    // Find the package
    let pkg = db
        .get_package_by_name(&args.package)?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: args.package.clone(),
        })?;

    // Check if already pinned
    if db.is_pinned(&pkg.id.name)? {
        println!("{} is already pinned.", pkg.id.name);
        return Ok(());
    }

    db.pin_package(&pkg.id.name, &pkg.id.version)?;
    println!(
        "Pinned {}-{} (will not be updated).",
        pkg.id.name, pkg.id.version
    );

    Ok(())
}

pub fn handle_unpin(args: UnpinArgs, db: &ZlDatabase) -> ZlResult<()> {
    if !db.is_pinned(&args.package)? {
        println!("{} is not pinned.", args.package);
        return Ok(());
    }

    db.unpin_package(&args.package)?;
    println!("Unpinned {} (updates allowed).", args.package);

    Ok(())
}

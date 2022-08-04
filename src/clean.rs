use crate::workspace::CargoContext;

pub fn clean_artifacts(ctx: CargoContext) -> anyhow::Result<()> {
    let pgo_dir = ctx.get_pgo_directory()?;
    let res = std::fs::remove_dir_all(pgo_dir);

    let bolt_dir = ctx.get_bolt_directory()?;
    std::fs::remove_dir_all(bolt_dir).and(res)?;

    Ok(())
}

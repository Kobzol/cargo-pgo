use crate::workspace::CargoContext;

pub fn clean_artifacts(ctx: CargoContext) -> anyhow::Result<()> {
    let pgo_directory = ctx.get_pgo_directory()?;
    let pgo_result = ctx.clean_pgo_directory(&pgo_directory);
    let (pgo_reset, pgo_error) = match pgo_result {
        Ok(reset) => (reset, None),
        Err(error) if ctx.pgo_profiles_are_custom() => return Err(error.into()),
        Err(error) => (None, Some(error)),
    };

    let bolt_directory = ctx.get_bolt_directory()?;
    let bolt_result = ctx.clean_bolt_directory(&bolt_directory);
    let (bolt_reset, bolt_error) = match bolt_result {
        Ok(reset) => (reset, None),
        Err(error) if ctx.bolt_profiles_are_custom() => return Err(error.into()),
        Err(error) => (None, Some(error)),
    };

    if let Some(error) = bolt_error.or(pgo_error) {
        return Err(error.into());
    }

    if let Some(reset) = pgo_reset {
        reset.finish(Ok(()))?;
    }
    if let Some(reset) = bolt_reset {
        reset.finish(Ok(()))?;
    }

    Ok(())
}

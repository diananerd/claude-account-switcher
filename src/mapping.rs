//! Which directory uses which profile: use, forget, status, resolve, map, prune.

use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::state::{self, Env, Hit, Result, Source};
use crate::{aborted, dir_or_cwd, note_running_session, paths, require, session_profile, target_dir, ui};

fn describe_source(env: &Env, hit: &Hit, dir: &std::path::Path) -> String {
    let here = paths::canonical(dir).as_deref() == Some(hit.key.as_path());
    match (hit.source, here) {
        (Source::Map, true) => "mapped here".into(),
        (Source::Map, false) => format!("inherited from {}", env.tilde(&hit.key)),
        (Source::File, true) => format!("pinned here by {}", state::LOCAL_FILE),
        (Source::File, false) => format!("pinned by {}/{}", env.tilde(&hit.key), state::LOCAL_FILE),
    }
}

pub fn use_profile(
    env: &Env,
    profile: Option<String>,
    dir: Option<PathBuf>,
    local: bool,
    prompt: bool,
) -> Result<ExitCode> {
    let cfg = env.load()?;
    let target = target_dir(dir)?;
    let name = match profile {
        Some(p) => p,
        None if prompt => {
            let current = cfg.lookup(&target).map(|h| h.profile);
            ui::pick_profile(env, &cfg, &format!("Profile for {}", env.tilde(&target)), current.as_deref())
                .ok_or_else(aborted)?
        }
        None => return Err("usage: claude-account use <profile> [dir]".into()),
    };
    require(&cfg, &name)?;
    if local {
        let file = target.join(state::LOCAL_FILE);
        fs::write(&file, format!("{name}\n")).map_err(|e| format!("cannot write {}: {e}", env.tilde(&file)))?;
        ui::done(&format!("{} -> {name} (pinned by {})", env.tilde(&target), state::LOCAL_FILE));
    } else {
        env.update(|c| {
            c.map.insert(target.clone(), name.clone());
            Ok(())
        })?;
        ui::done(&format!("{} -> {name}", env.tilde(&target)));
    }
    if state::covers_home(env, &target) {
        eprintln!(
            "Note: {} contains your home folder, so every folder in it without a closer mapping uses {name}.",
            env.tilde(&target)
        );
    }
    eprintln!("Applies to new sessions; a running session keeps its account.");
    note_running_session(env);
    Ok(ExitCode::SUCCESS)
}

pub fn forget(env: &Env, dir: Option<PathBuf>, local: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    // A mapped folder that no longer exists (or sits on an unmounted volume) can
    // still be forgotten by naming it.
    let target = match (target_dir(dir.clone()), &dir) {
        (Ok(t), _) => t,
        (Err(e), Some(d)) => {
            let base = crate::cwd().unwrap_or_else(|_| env.home.clone());
            let abs = paths::canonical_lenient(&paths::lexical_absolute(d, &base));
            if !local && cfg.map.contains_key(&abs) { abs } else { return Err(e) }
        }
        (Err(e), None) => return Err(e),
    };
    if local {
        let file = target.join(state::LOCAL_FILE);
        if !file.exists() {
            return Err(format!("{} has no {}", env.tilde(&target), state::LOCAL_FILE));
        }
        fs::remove_file(&file).map_err(|e| format!("cannot remove {}: {e}", env.tilde(&file)))?;
    } else {
        if !cfg.map.contains_key(&target) {
            return Err(match cfg.lookup(&target) {
                Some(h) if h.source == Source::File && h.key == target => format!(
                    "{} is pinned by {}; remove it with: claude-account forget --local",
                    env.tilde(&target),
                    state::LOCAL_FILE
                ),
                Some(h) => format!(
                    "{} has no mapping of its own: it inherits {} from {}",
                    env.tilde(&target),
                    h.profile,
                    env.tilde(&h.key)
                ),
                None => format!("{} is not mapped", env.tilde(&target)),
            });
        }
        env.update(|c| {
            c.map.remove(&target);
            Ok(())
        })?;
    }
    match env.load()?.lookup(&target) {
        Some(h) => {
            println!("{} now resolves to {} ({})", env.tilde(&target), h.profile, describe_source(env, &h, &target))
        }
        None => println!("{} is no longer mapped: the picker will ask next time", env.tilde(&target)),
    }
    Ok(ExitCode::SUCCESS)
}

pub fn status(env: &Env, dir: Option<PathBuf>, json: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    let dir = dir_or_cwd(dir)?;
    let canon = paths::canonical(&dir).unwrap_or_else(|| dir.clone());
    let hit = cfg.lookup(&dir);
    let session = std::env::var_os("CLAUDECODE").and_then(|_| session_profile(env, &cfg, &dir));
    if json {
        let v = json!({
            "directory": canon,
            "profile": hit.as_ref().map(|h| &h.profile),
            "matched": hit.as_ref().map(|h| &h.key),
            "source": hit.as_ref().map(|h| match h.source { Source::Map => "map", Source::File => "file" }),
            "inherited": hit.as_ref().map(|h| h.key != canon),
            "default": cfg.default_name(),
            "session_profile": session,
        });
        println!("{v:#}");
        return Ok(ExitCode::SUCCESS);
    }
    println!("Folder:    {}", env.tilde(&canon));
    match &hit {
        Some(h) => {
            let known = cfg.exists(&h.profile);
            let desc = if known { ui::describe(env, &cfg, &h.profile) } else { "unknown profile".into() };
            println!("Profile:   {} ({desc})", h.profile);
            println!("Source:    {}", describe_source(env, h, &dir));
        }
        None => match cfg.default_name() {
            Some(d) => println!("Profile:   not mapped; the picker will offer {d}"),
            None => println!("Profile:   none yet; run: claude-account setup"),
        },
    }
    if let (Some(s), Some(h)) = (&session, &hit)
        && *s != h.profile
    {
        println!("Session:   runs as {s}; relaunch claude to switch (claude --continue resumes)");
    }
    Ok(ExitCode::SUCCESS)
}

pub fn resolve(env: &Env, dir: Option<PathBuf>, json: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    let hit = cfg.lookup(&dir_or_cwd(dir)?);
    if json {
        println!("{}", json!({"profile": hit.as_ref().map(|h| &h.profile), "matched": hit.as_ref().map(|h| &h.key)}));
    } else {
        println!("{}", hit.map(|h| h.profile).unwrap_or_default());
    }
    Ok(ExitCode::SUCCESS)
}

pub fn map(env: &Env, json: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    if json {
        println!("{:#}", json!(cfg.map));
        return Ok(ExitCode::SUCCESS);
    }
    if cfg.map.is_empty() {
        println!("(nothing mapped yet)");
    }
    let width = cfg.map.values().map(String::len).max().unwrap_or(0);
    for (k, v) in &cfg.map {
        let note = match state::reach(k) {
            state::Reach::Present => "",
            state::Reach::Deleted => "  (deleted; see: claude-account prune)",
            state::Reach::Unreachable => "  (not reachable: unmounted volume or moved parent?)",
        };
        println!("{v:width$}  {}{note}", env.tilde(k));
    }
    Ok(ExitCode::SUCCESS)
}

pub fn prune(env: &Env, all: bool, mode: crate::Mode) -> Result<ExitCode> {
    let cfg = env.load()?;
    let (mut drop, mut kept) = (vec![], vec![]);
    for k in cfg.map.keys() {
        match state::reach(k) {
            state::Reach::Present => {}
            state::Reach::Deleted => drop.push(k.clone()),
            state::Reach::Unreachable if all => drop.push(k.clone()),
            state::Reach::Unreachable => kept.push(k.clone()),
        }
    }
    for k in &kept {
        eprintln!("Kept {} (not reachable, may come back; --all drops it too)", env.tilde(k));
    }
    if drop.is_empty() {
        println!("Nothing to prune.");
        return Ok(ExitCode::SUCCESS);
    }
    if mode.prompt && !mode.yes {
        eprintln!("Mappings to folders that are gone:");
        for k in &drop {
            eprintln!("  {}  ({})", env.tilde(k), cfg.map[k]);
        }
        if ui::confirm(&format!("Drop these {}?", drop.len()), true) != Some(true) {
            return Err(crate::aborted());
        }
    }
    env.update(|c| {
        for k in &drop {
            c.map.remove(k);
        }
        Ok(())
    })?;
    for g in drop {
        ui::done(&format!("Dropped {}", env.tilde(&g)));
    }
    Ok(ExitCode::SUCCESS)
}

use std::process::Command;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{State, Manager, Emitter};
use std::sync::Arc;
use axum::{
    routing::post,
    Router,
    Json,
    extract::State as AxumState,
};
use tauri_plugin_notification::NotificationExt;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

fn create_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Worktree {
    path: String,
    head_hash: String,
    branch: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct HookPayload {
    path: String,
    status: String, // "waiting_auth", "running", "idle"
    message: Option<String>
}

struct ServerState {
    app_handle: tauri::AppHandle,
}

fn parse_worktrees(output: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut current_worktree = Worktree {
        path: String::new(),
        head_hash: String::new(),
        branch: None,
    };
    let mut has_data = false;

    for line in output.lines() {
        if line.is_empty() {
             if has_data {
                worktrees.push(current_worktree);
                current_worktree = Worktree {
                    path: String::new(),
                    head_hash: String::new(),
                    branch: None,
                };
                has_data = false;
             }
             continue;
        }
        
        if line.starts_with("worktree ") {
             current_worktree.path = line[9..].to_string();
             has_data = true;
        } else if line.starts_with("HEAD ") {
             current_worktree.head_hash = line[5..].to_string();
        } else if line.starts_with("branch ") {
             let ref_name = &line[7..];
             let branch_name = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
             current_worktree.branch = Some(branch_name.to_string());
        }
    }
    if has_data {
        worktrees.push(current_worktree);
    }
    worktrees
}

#[tauri::command]
fn list_worktrees(project_path: String) -> Result<Vec<Worktree>, String> {
    let output = create_command("git")
        .arg("worktree")
        .arg("list")
        .arg("--porcelain")
        .current_dir(&project_path)
        .output()
        .map_err(|e| format!("Failed to execute git command at '{}': {}", project_path, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git Error at '{}': {}", project_path, stderr));
    }

    Ok(parse_worktrees(&String::from_utf8_lossy(&output.stdout)))
}

use walkdir::{WalkDir, DirEntry};
use std::collections::HashSet;

fn link_gitignored_items(project_path: &str, worktree_path: &str) {
    let project_dir = std::path::Path::new(project_path);
    let worktree_dir = std::path::Path::new(worktree_path);
    let gitignore_path = project_dir.join(".gitignore");

    println!("Scanning for recursive ignore targets via .gitignore...");

    let mut ignore_targets = HashSet::new();
    // Default commons just in case (optional, but requested based on .gitignore)
    // ignore_targets.insert("node_modules".to_string());
    // ignore_targets.insert(".env".to_string());

    if gitignore_path.exists() {
         if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
             for line in content.lines() {
                 let line = line.trim();
                 if line.is_empty() || line.starts_with('#') { continue; }
                 
                 // Clean up pattern: /node_modules -> node_modules, node_modules/ -> node_modules
                 let clean = line.replace('\\', "/");
                 let clean = clean.trim_matches('/');
                 
                 // We collect names. Simple logic: if a dir matches this name, we link it.
                 // This covers standard "node_modules" rules.
                 // Complex globs like "*.log" are harder with this approach but simple names cover 90% of cases.
                 if !clean.is_empty() {
                    ignore_targets.insert(clean.to_string());
                 }
             }
         }
    } else {
        println!("No .gitignore found, skipping auto-link.");
        return;
    }
    
    if ignore_targets.is_empty() {
        return;
    }

    println!("Targets to link: {:?}", ignore_targets);

    // Recursive walk
    let walker = WalkDir::new(project_dir).min_depth(1).into_iter();
    
    // We strictly want to skip descending into the directories we link, 
    // AND skip hidden git dirs to save time.
    fn is_ignored(entry: &DirEntry, targets: &HashSet<String>) -> bool {
        entry.file_name()
             .to_str()
             .map(|s| targets.contains(s) || s == ".git" || s == ".vs" || s == "node_modules") 
             // We check node_modules explicitly here to ensure we don't scan INSIDE a source node_modules if we missed adding it to targets
             .unwrap_or(false)
    }

    // Custom filtering loop to control skipping
    let mut it = walker.filter_entry(|e| !is_ignored(e, &ignore_targets));

    // Wait, filter_entry skips descending but DOES yield the entry itself.
    // Actually we want:
    // 1. Visit entry.
    // 2. If entry matches target -> Link it -> Skip recursion.
    // 3. If entry is .git -> Skip recursion.
    // 4. Else -> recurse.
    
    // WalkDir's filter_entry is: if false, skip directory.
    // But we need to distinguish "Link this dir and stop" vs "Ignore this dir".
    // Let's iterate manually using standard recursive iterator logic or just loop WalkDir without filter_entry 
    // and manually skip? No, WalkDir provides skip_current_dir().

    let mut iterator = WalkDir::new(project_dir).min_depth(1).into_iter();

    // Collect failed links to run in batch
    let mut pending_admin_links = Vec::new();

    loop {
        let entry = match iterator.next() {
            None => break,
            Some(Err(_)) => continue,
            Some(Ok(e)) => e,
        };

        let file_name = entry.file_name().to_string_lossy().to_string();
        
        // Safety skip
        if file_name == ".git" {
            iterator.skip_current_dir();
            continue;
        }

        if ignore_targets.contains(&file_name) {
            let src_path = entry.path();
            
            if let Ok(rel) = src_path.strip_prefix(project_dir) {
                let dest_path = worktree_dir.join(rel);
                
                if let Some(parent) = dest_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                if !dest_path.exists() {
                     #[cfg(target_os = "windows")]
                     {
                        if src_path.is_dir() {
                             let dest_str = dest_path.display().to_string().replace("/", "\\");
                             let src_str = src_path.display().to_string().replace("/", "\\");
                             let cmd_str = format!("mklink /J \"{}\" \"{}\"", dest_str, src_str);
                             
                             let output = create_command("cmd").arg("/C").arg(&cmd_str).output();
                             let mut success = false;
                             if let Ok(o) = output {
                                 if o.status.success() {
                                     println!("Linked: {} -> {}", src_path.display(), dest_path.display());
                                     success = true;
                                 }
                             }
                             
                             if !success {
                                 println!("Requires Admin: {}", dest_path.display());
                                 pending_admin_links.push(cmd_str);
                             }
                        } else {
                            // File Fallback Chain
                            let _ = std::os::windows::fs::symlink_file(src_path, &dest_path)
                                .or_else(|_| std::fs::hard_link(src_path, &dest_path))
                                .or_else(|_| std::fs::copy(src_path, &dest_path).map(|_| ()));
                        }
                     }
                }
            }
            if entry.file_type().is_dir() {
                iterator.skip_current_dir();
            }
        }
    }
    
    // Batch Execute Admin Links
    if !pending_admin_links.is_empty() {
        println!("requesting admin for {} items...", pending_admin_links.len());
        
        // Use PowerShell script instead of Batch to handle Encoding/Unicode correctly.
        // We prepend the UTF-8 BYTE ORDER MARK (BOM) so PowerShell explicitly knows it's UTF-8.
        let mut ps1_content = String::from("\u{FEFF}"); 
        ps1_content.push_str("$ErrorActionPreference = 'Stop'\n");

        for cmd in pending_admin_links {
            // cmd contains: mklink /J "dest" "src"
            // In PowerShell, we run this via cmd /c. 
            // We need to escape quotes if necessary, but usually single quotes around the whole string works best in PS.
            // Example: cmd /c 'mklink /J "dest" "src"'
            
            // However, our cmd string already has quotes. 
            // Let's rely on PS parsing. 
            // cmd /c mklink /J "D:\..." "D:\..."
            // In PS script: cmd /c $cmd  -- wait, $cmd needs to be exact.
            
            // outputting: cmd /c "mklink /J \"dest\" \"src\""
            
            // Simplest way: Write exact command line.
            // cmd /c $cmd
            
            ps1_content.push_str(&format!("cmd /c '{}'\n", cmd));
        }
        
        ps1_content.push_str("Write-Host 'Press Key to exit...'\n");
        ps1_content.push_str("$null = $Host.UI.RawUI.ReadKey('NoEcho,IncludeKeyDown')\n");
        
        let temp_dir = std::env::temp_dir();
        let ps1_path = temp_dir.join("git_worktree_links.ps1");
        
        if std::fs::write(&ps1_path, ps1_content).is_ok() {
            let ps1_path_str = ps1_path.display().to_string();
            
            // Run PowerShell as Admin, executing the generated script
            let _ = create_command("powershell")
                .arg("-Command")
                .arg(format!("Start-Process powershell -Verb RunAs -ArgumentList '-ExecutionPolicy Bypass -File \"{}\"' -Wait", ps1_path_str))
                .output();
        }
    }
}

#[tauri::command]
fn create_worktree(project_path: String, path: String, branch: String, base: Option<String>, smart_sync: bool) -> Result<(), String> {
    let mut cmd = create_command("git");
    cmd.current_dir(&project_path)
       .arg("worktree")
       .arg("add")
       .arg("-b")
       .arg(&branch)
       .arg(&path);
    
    if let Some(b) = base {
        cmd.arg(b);
    }
    
    let output = cmd.output().map_err(|e| e.to_string())?;
    
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // Auto-link gitignored files if enabled
    if smart_sync {
        link_gitignored_items(&project_path, &path);
    }

    Ok(())
}

#[tauri::command]
fn remove_worktree(project_path: String, worktree_path: String, branch: Option<String>) -> Result<(), String> {
    // 1. Remove Worktree
    // We use --force, but if it fails (e.g. locked files or strange junctions), we manual clean.
    let output = create_command("git")
        .current_dir(&project_path)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(&worktree_path)
        .output()
        .map_err(|e| e.to_string())?;

    // Even if git fails (e.g. "not empty"), we try to force remove the directory manually
    // because we know we created junctions that git might choke on.
    let path_obj = std::path::Path::new(&worktree_path);
    if path_obj.exists() {
        println!("Git remove finished (success={}), but dir exists. Force removing: {}", output.status.success(), worktree_path);
        // std::fs::remove_dir_all is safe for symlinks/junctions (does not follow)
        if let Err(e) = std::fs::remove_dir_all(path_obj) {
            println!("Failed to force remove directory: {}", e);
            // If git failed AND we failed to delete, then return error.
            if !output.status.success() {
                 return Err(format!("Git failed: {}; Force remove failed: {}", String::from_utf8_lossy(&output.stderr), e));
            }
        }
    } else if !output.status.success() {
         // Dir gone but git reported error?
         return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // Run prune to clean up any stale git metadata if git command failed but we deleted dir
    let _ = create_command("git").current_dir(&project_path).arg("worktree").arg("prune").output();

    // 2. Delete Branch if provided
    if let Some(branch_name) = branch {
         // Only attempt delete if branch is valid
         if !branch_name.is_empty() {
             let branch_output = create_command("git")
                .current_dir(&project_path)
                .arg("branch")
                .arg("-D")
                .arg(&branch_name)
                .output()
                .map_err(|e| e.to_string())?;
                
             if !branch_output.status.success() {
                 return Err(format!("Worktree removed, but failed to delete branch '{}': {}", branch_name, String::from_utf8_lossy(&branch_output.stderr)));
             }
         }
    }
    Ok(())
}

#[tauri::command]
fn open_worktree_dir(path: String) -> Result<(), String> {
    println!("Attempting to open path: '{}'", path);
    // Try cleaning up the path for Windows
    #[cfg(target_os = "windows")]
    let path = path.replace("/", "\\");

    println!("Normalized path: '{}'", path);

    opener::open(&path).map_err(|e| {
        println!("Opener error: {}", e);
        format!("Failed to open folder '{}': {}", path, e)
    })?;
    Ok(())
}

async fn hook_handler(
    AxumState(state): AxumState<Arc<ServerState>>,
    Json(payload): Json<HookPayload>,
) {
    println!("Received hook: {} - {}", payload.path, payload.status);
    use tauri::Emitter;
    let _ = state.app_handle.emit("claude-status-change", &payload);
    
    if payload.status == "waiting_auth" {
         let _ = state.app_handle.notification()
            .builder()
            .title("Claude Permission Request")
            .body("Claude is waiting for your approval.")
            .show();
    }
}

#[tauri::command]
fn install_claude_hooks() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let user_profile = std::env::var("USERPROFILE").map_err(|_| "Could not find USERPROFILE")?;
        let claude_dir = std::path::Path::new(&user_profile).join(".claude");
        let settings_path = claude_dir.join("settings.json");
        let hooks_dir = claude_dir.join("hooks");
        
        if !claude_dir.exists() {
             std::fs::create_dir_all(&claude_dir).map_err(|e| e.to_string())?;
        }
        if !hooks_dir.exists() {
             std::fs::create_dir_all(&hooks_dir).map_err(|e| e.to_string())?;
        }
        
        // 1. Write Hook Script
        let hook_script_path = hooks_dir.join("git-worktree-hook.ps1");
        let script_content = r#"
param (
    [string]$Type
)

$Path = Get-Location
$Payload = @{
    path = $Path.Path
    status = "idle"
    message = ""
}

switch ($Type) {
    "PermissionRequest" { $Payload.status = "waiting_auth" }
    "PreToolUse" { $Payload.status = "running" }
    "PostToolUse" { $Payload.status = "running" }
    "Stop" { $Payload.status = "idle" }
}

try {
    Invoke-RestMethod -Uri "http://localhost:36911/claude/status" -Method Post -Body ($Payload | ConvertTo-Json) -ContentType "application/json" -ErrorAction SilentlyContinue
} catch {}
"#;
        std::fs::write(&hook_script_path, script_content).map_err(|e| e.to_string())?;
        
        // 2. Update settings.json
        // This is tricky without a JSON parser crate helper, but I imported serde_json
        // I need to read existing or create new.
        let mut settings: serde_json::Value = if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
            serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
        } else {
             serde_json::json!({})
        };
        
        if settings.get("hooks").is_none() {
            settings["hooks"] = serde_json::json!({});
        }
        
        // Use ampersand execution operator which handles quoted paths better in some contexts
        // Also stick to backslashes but escape them for JSON string
        let path_str = hook_script_path.to_string_lossy().to_string();
        let cmd_base = format!("powershell -ExecutionPolicy Bypass -Command \"& '{}' -Type\"", path_str);
        
        // Helper to create the new hook structure: [{ "hooks": [{ "type": "command", "command": "..." }] }]
        // We omit "matcher" to apply to all events of that type
        let make_hook = |event_type: &str| {
            serde_json::json!([
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{} '{}'\"", cmd_base, event_type)
                        }
                    ]
                }
            ])
        };

        settings["hooks"]["PermissionRequest"] = make_hook("PermissionRequest");
        settings["hooks"]["PreToolUse"] = make_hook("PreToolUse");
        settings["hooks"]["PostToolUse"] = make_hook("PostToolUse");
        settings["hooks"]["Stop"] = make_hook("Stop");
        
        let new_content = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        std::fs::write(&settings_path, new_content).map_err(|e| e.to_string())?;
    }
    Ok(())
}

struct ClaudeState(Mutex<HashMap<String, u32>>);

#[tauri::command]
fn open_claude(path: String, state: State<'_, ClaudeState>) -> Result<(), String> {
    println!("Opening Claude in: {}", path);
    
    // Check if already running (basic check)
    let mut session_map = state.0.lock().map_err(|_| "Failed to lock state")?;
    
    // Spawn PowerShell with Start-Process to ensure new window
    // We use -PassThru to get process info back, and Select-Object -ExpandProperty Id to get the PID
    #[cfg(target_os = "windows")]
    let output = create_command("powershell")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(format!(
            "Start-Process powershell -ArgumentList '-NoExit', '-Command', \"Set-Location -LiteralPath '{}'; claude\" -PassThru | Select-Object -ExpandProperty Id", 
            path
        ))
        .output()
        .map_err(|e| e.to_string())?;

    #[cfg(not(target_os = "windows"))]
    return Err("Claude integration currently only supports Windows".to_string());
    
    #[cfg(target_os = "windows")]
    {
        if !output.status.success() {
             return Err(format!("Failed to spawn process: {}", String::from_utf8_lossy(&output.stderr)));
        }
        
        // Parse PID from stdout
        let stdout = String::from_utf8_lossy(&output.stdout);
        let pid_str = stdout.trim();
        println!("Claude spawned with PID: '{}'", pid_str);
        
        if let Ok(pid) = pid_str.parse::<u32>() {
            session_map.insert(path, pid);
        } else {
             return Err(format!("Failed to parse PID from '{}'", pid_str));
        }
    }
    
    Ok(())
}

#[tauri::command]
fn focus_claude(path: String, state: State<'_, ClaudeState>) -> Result<(), String> {
    let mut session_map = state.0.lock().map_err(|_| "Failed to lock state")?;
    
    if let Some(&pid) = session_map.get(&path) {
         println!("Focusing PID: {}", pid);
         let script = format!("(New-Object -ComObject WScript.Shell).AppActivate({})", pid);
         #[cfg(target_os = "windows")]
         let output = create_command("powershell")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(&script)
            .output()
            .map_err(|e| e.to_string())?;

         #[cfg(not(target_os = "windows"))]
         return Err("Not supported".to_string());
            
         let out_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
         println!("Focus result for PID {}: '{}'", pid, out_str);
         
         if out_str == "False" {
             println!("Focus returned False (window might be already active or prevented). Treating as success to avoid UI error since user reports it works.");
             // Do not return Err, just Ok. Polling will clean up if it's actually dead.
             return Ok(());
         }
         Ok(())
    } else {
        Err("No active Claude session found for this path".to_string())
    }
}

#[tauri::command]
fn list_claude_sessions(state: State<'_, ClaudeState>) -> Result<Vec<String>, String> {
    let mut session_map = state.0.lock().map_err(|_| "Failed to lock state")?;
    
    // Cleanup dead sessions logic could go here
    // iterate and check if PID is alive.
    // For Windows, "tasklist /FI 'PID eq <pid>'" is one way, or just Get-Process
    
    let mut dead_paths = Vec::new();
    
    for (path, &pid) in session_map.iter() {
         #[cfg(target_os = "windows")]
         {
             let check = create_command("powershell")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(format!("Get-Process -Id {} -ErrorAction SilentlyContinue", pid))
                .output();
             
             match check {
                 Ok(output) => {
                     if !output.status.success() {
                         // Likely dead
                         println!("PID {} check failed (status), marking dead", pid);
                         dead_paths.push(path.clone());
                     }
                 },
                 Err(e) => {
                     println!("PID {} check error: {}, marking dead", pid, e);
                     dead_paths.push(path.clone());
                 }
             }
         }
    }
    
    for p in dead_paths {
        println!("Removing dead session: {}", p);
        session_map.remove(&p);
    }

    Ok(session_map.keys().cloned().collect())
}

#[tauri::command]
fn kill_claude_session(path: String, state: State<'_, ClaudeState>) -> Result<(), String> {
    let mut session_map = state.0.lock().map_err(|_| "Failed to lock state")?;
    
    if let Some(&pid) = session_map.get(&path) {
         println!("Killing session for path: {} (PID: {})", path, pid);
         
         #[cfg(target_os = "windows")]
         {
             // Use taskkill /F /PID <pid> /T to force kill tree (including window)
             let output = create_command("taskkill")
                .arg("/F")
                .arg("/T")
                .arg("/PID")
                .arg(pid.to_string())
                .output()
                .map_err(|e| e.to_string())?;
                
             if !output.status.success() {
                 let stderr = String::from_utf8_lossy(&output.stderr);
                 // If process already gone (128), that's fine too
                 println!("taskkill warning: {}", stderr);
             }
         }

         session_map.remove(&path);
         Ok(())
    } else {
        // Algorithmically successful if it's already gone
        Ok(())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_notification::init())
    .manage(ClaudeState(Mutex::new(HashMap::new())))
    .invoke_handler(tauri::generate_handler![list_worktrees, create_worktree, remove_worktree, open_worktree_dir, open_claude, focus_claude, list_claude_sessions, kill_claude_session, install_claude_hooks])
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      // Start local Hook Server
      let app_handle = app.handle().clone();
      tauri::async_runtime::spawn(async move {
          let state = Arc::new(ServerState { app_handle });
          
          let router = Router::new()
              .route("/claude/status", post(hook_handler))
              .with_state(state);
              
          // Silent unwrap for now, assumes port 36911 is free.
          if let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:36911").await {
              println!("Hook server running on 36911");
              let _ = axum::serve(listener, router).await;
          } else {
              eprintln!("Failed to bind port 36911 for Hook Server");
          }
      });
      
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

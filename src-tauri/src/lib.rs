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
    let output = Command::new("git")
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

#[tauri::command]
fn create_worktree(project_path: String, path: String, branch: String, base: Option<String>) -> Result<(), String> {
    let mut cmd = Command::new("git");
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
    Ok(())
}

#[tauri::command]
fn remove_worktree(project_path: String, worktree_path: String, branch: Option<String>) -> Result<(), String> {
    // 1. Remove Worktree
    let output = Command::new("git")
        .current_dir(&project_path)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(&worktree_path)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // 2. Delete Branch if provided
    if let Some(branch_name) = branch {
         // Only attempt delete if branch is valid
         if !branch_name.is_empty() {
             let branch_output = Command::new("git")
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
    let output = Command::new("powershell")
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
         let output = Command::new("powershell")
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
             let check = Command::new("powershell")
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
             let output = Command::new("taskkill")
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

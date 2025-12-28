use std::process::Command;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Worktree {
    path: String,
    head_hash: String,
    branch: Option<String>,
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

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::State;

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
    .manage(ClaudeState(Mutex::new(HashMap::new())))
    .invoke_handler(tauri::generate_handler![list_worktrees, create_worktree, remove_worktree, open_worktree_dir, open_claude, focus_claude, list_claude_sessions, kill_claude_session])
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

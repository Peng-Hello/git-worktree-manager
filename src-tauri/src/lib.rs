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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .invoke_handler(tauri::generate_handler![list_worktrees, create_worktree, remove_worktree, open_worktree_dir])
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

# Git Worktree Manager

<div align="center">

![Git Worktree Manager](./assets/02.png)

**Modern Git Worktree Management GUI built with Tauri + Vue 3**  
*Say goodbye to `git stash`, embrace parallel multi-branch development workflows*

[中文版](./README.md)

</div>

## 📖 Why this tool?

**Core Pain Point: How to elegantly have multiple AI Agents (Claude Code) work on one project simultaneously?**

With the rise of AI-assisted programming, we often need multiple Claude Code instances to work on different tasks in parallel (e.g., Agent A fixing bugs, Agent B writing new features, Agent C refactoring). However, if they all operate in the same project directory:
- **File Conflicts**: Multiple AIs modifying files simultaneously leads to overwrites or Git locking.
- **Context Pollution**: Agent A reads Agent B's half-finished code, leading to hallucinations or logic errors.
- **Environment Clashes**: Multiple processes fighting for build locks or ports.

**Git Worktree is the "Physical Isolation Chamber" for this scenario.** It allows us to check out multiple **independent filesystem working areas** from the same repository.

**Git Worktree Manager is essentially an orchestration terminal for Multi-Agent Collaboration.**
It's not just about managing Git branches; it's about **physically isolating** working environments for multiple AI agents. You can easily create a "sandbox" for each task, allowing Claude to excel without "fighting" over file conflicts. Think of it as your **AI Engineer Team's Workspace Manager**.

## ✨ Core Features

### 🌳 Visual Workspace Management
- View all Worktrees associated with the current repository at a glance.
- Intuitively display branch names, commit hashes, and path information.
- Automatically identify and manage "wild" Worktrees.

### ⚡ Rapid Creation & Destruction
- **One-click Creation**: Just enter a new branch name (e.g., `feat/login`), and a peer directory is automatically created.
- **Safe Cleanup**: After development, click Remove to safely delete the branch and folder together, keeping your disk clean.

### 🤖 Claude Code Deep Integration
Deeply integrated experience designed for AI-assisted programming:
- **Embedded Terminal**: Launch Claude Code directly in the corresponding Worktree directory.
- **AI-Driven Status Awareness**: Cards show Claude's real-time status:
    - 🟡 **Waiting for Approval**: Card highlights and sends system notification when Claude requests permissions.
    - 🔵 **Working**: AI is thinking or executing tasks.
    - 🟢 **Idle**: Task completed, standing by.
- **Auto Configuration**: Works out of the box, automatically configuring Claude Hooks without manual script tweaking.

### 🔗 Smart Dependency Sync (New!)
Specially designed for large Monorepo projects like Vben Admin:
- **Recursive Identification**: Automatically reads the main repo's `.gitignore` and recursively scans/links all ignored directories (e.g., `apps/*/node_modules`, `.env`, `target`).
- **Zero Disk Space**: Uses Junction/Symlink technology—Worktrees are generated in seconds without consuming extra disk space.
- **Optional Toggle (New)**: An optional switch (default off) in the creation modal, allowing you to decide when to link `node_modules`.
- **Automated Admin Batching**: If permission issues occur (e.g., Windows restricting link creation), all requests are collected into a **single UAC prompt** to get everything done at once.

## ⚠️ Important Notes

- **Windows Only**: Currently, the core logic (like `mklink` and `RunAs` admin elevation) deeply relies on Windows mechanisms. macOS or Linux are not supported yet.
- **Avoid Cross-Drive Worktrees**: While there's a fallback to copying, cross-drive links prevent Hard Links/Junctions. Huge `node_modules` will be physically copied, which is slow and consumes double space. **Recommended to set the Root Directory on the same disk partition as the main repository.**

## 📷 Screenshots

| Dashboard | Claude Integration |
|:---:|:---:|
| ![Dashboard](./assets/01.png) | ![Claude Integration](./assets/02.png) |

> *The interface features a clean card layout with status changes during Claude Code interaction.*

## 🛠️ Tech Stack

- **Core**: [Tauri 2.0](https://tauri.app/) (Rust) - Extremely lightweight and secure
- **Frontend**: Vue 3 + TypeScript + TailwindCSS - Smooth interaction experience
- **AI Integration**: Rust Axum Server + PowerShell Hooks

## 🚀 Quick Start

### 1. Install Dependencies
```bash
npm install
```

### 2. Run in Dev Mode
```bash
npm run tauri dev
```
*Note: On Windows, the Claude Hooks script will be automatically installed to your user directory on first launch.*

### 3. Build Production Package
```bash
npm run tauri build
```

## 📝 Usage Guide

1.  **Select Repository**: Click the top-right button to select your main Git repository directory.
2.  **Set Root Directory**: Choose a folder to store all new Worktrees (recommended to be a peer of the main repo).
3.  **New Workspace**: Click "New Worktree", enter a branch name, and start parallel development instantly.
4.  **Call Claude**: Click the "Claude" button on the card to summon the AI assistant; it will automatically enter that directory to write code for you.

## 📄 License

MIT License - Copyright (c) 2025 Peng-Hello

<script setup lang="ts">
import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Worktree } from "./types";

const projectPath = ref<string>("");
const globalRoot = ref<string>(""); // Global root for new worktrees
const worktrees = ref<Worktree[]>([]);
const errorMsg = ref<string>("");
const loading = ref(false);

// New Worktree Form
const showModal = ref(false);
const newBranch = ref("");
const baseBranch = ref("main");

// Computed target path: GlobalRoot / ProjectName-BranchName
const computedPreviewPath = computed(() => {
    if (!globalRoot.value || !newBranch.value) return "";
    const projectName = projectPath.value ? projectPath.value.split(/[\\/]/).pop() : "Repo";
    const safeBranch = newBranch.value.replace(/[\/\\]/g, "-");
    return `${globalRoot.value}/${projectName}-${safeBranch}`;
});

// Filter worktrees:
// 1. Must be inside Global Root (if set)
// 2. Folder name should roughly match ProjectName- (Optional, but user mentioned "matching rule")
const visibleWorktrees = computed(() => {
    if (!globalRoot.value) return worktrees.value;
    
    // Normalize global root for comparison (simple check)
    // In a real app we'd use a robust path lib, but here we strip trailing slash and case insensitive on windows.
    const normalizedRoot = globalRoot.value.replace(/[\\/]$/, "").toLowerCase();
    
    return worktrees.value.filter(wt => {
        const wtPath = wt.path.replace(/[\\/]/g, "/"); // normalize to forward slashes for easier parsing if mixed
        // We can just rely on checking if the path string includes the root.
        // But better: check if it starts with root.
        
        // Note: wt.path comes from git, might be full path. 
        // globalRoot comes from dialog, is full path.
        
        const normalizedWtPath = wt.path.replace(/[\\/]/g, "/").toLowerCase();
        
        // Check containment
        // We use forward slash normalized check
        const slashRoot = normalizedRoot.replace(/\\/g, "/");
        
        return normalizedWtPath.startsWith(slashRoot);
    });
});

async function selectProject() {
// ... existing selectProject ...

  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select Main Repository"
    });
    if (selected && typeof selected === "string") {
      projectPath.value = selected;
      loadWorktrees();
      
      // Auto-set Global Root to parent if not set
      // e.g. D:/Work/Repo/.git -> D:/Work/
      if (!globalRoot.value) {
           // Basic parent extraction
           const parts = selected.split(/[\\/]/);
           parts.pop(); // remove Repo
           globalRoot.value = parts.join("/");
      }
    }
  } catch (e) {
    errorMsg.value = "Failed to open dialog: " + e;
  }
}

async function selectGlobalRoot() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select Worktree Root Directory"
    });
    if (selected && typeof selected === "string") {
      globalRoot.value = selected;
    }
  } catch (e) {
    errorMsg.value = "Failed to open dialog: " + e;
  }
}

async function loadWorktrees() {
  if (!projectPath.value) return;
  loading.value = true;
  errorMsg.value = "";
  try {
    const res = await invoke("list_worktrees", { projectPath: projectPath.value });
    worktrees.value = res as Worktree[];
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function createWorktree() {
  if (!newBranch.value || !globalRoot.value) return;
  
  loading.value = true;
  errorMsg.value = "";
  try {
     // We use the computed path but need to make sure we clean it up for the OS if needed
     // Ideally pass separate args to backend, but backend expects full path.
     // We'll trust the computed string for now.
     
     await invoke("create_worktree", {
        projectPath: projectPath.value,
        path: computedPreviewPath.value,
        branch: newBranch.value,
        base: baseBranch.value || null
     });
     showModal.value = false;
     newBranch.value = "";
     // Reload
     loadWorktrees();
  } catch (e) {
     errorMsg.value = String(e);
  } finally {
     loading.value = false;
  }
}

async function openFolder(path: string) {
  try {
    await invoke("open_worktree_dir", { path });
  } catch (e) {
    errorMsg.value = "Failed to open folder: " + String(e);
  }
}

async function removeWorktree(path: string, branch?: string) {
  if (!confirm(`Are you sure you want to remove the worktree at:\n${path}\n\nThis will also FORCE DELETE the branch '${branch || 'unknown'}' and cannot be undone.`)) return;
  loading.value = true;
  try {
    await invoke("remove_worktree", { 
        projectPath: projectPath.value, 
        worktreePath: path,
        branch: branch
    });
    loadWorktrees();
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="min-h-screen bg-gray-50 text-gray-900 font-sans selection:bg-blue-100 selection:text-blue-900">
    <div class="max-w-6xl mx-auto p-6 lg:p-10">
      
      <!-- Header -->
      <header class="mb-10 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 class="text-4xl font-extrabold tracking-tight bg-clip-text text-transparent bg-gradient-to-br from-gray-900 to-gray-600">
            Git Worktree Manager
          </h1>
          <p class="text-gray-500 mt-2 text-lg">Streamline your multi-branch workflow</p>
        </div>
        
        <div class="flex gap-3">
             <!-- Global Root Selector -->
            <button 
                @click="selectGlobalRoot"
                class="group relative px-4 py-3 bg-white hover:bg-gray-50 border border-gray-200 rounded-xl shadow-sm hover:shadow-md transition-all duration-200 ease-out flex items-center gap-3 overflow-hidden"
                :class="{'border-red-300 ring-2 ring-red-100': !globalRoot && projectPath}"
            >
            <div class="flex flex-col items-start text-left max-w-[200px]">
                <span class="text-xs font-semibold text-gray-400 uppercase tracking-wider">Worktree Root</span>
                <span class="font-medium text-gray-700 truncate w-full" :title="globalRoot">{{ globalRoot ? globalRoot.split(/[\\/]/).pop() : 'Not Set' }}</span>
            </div>
            <svg class="w-5 h-5 text-gray-400 group-hover:text-blue-600 transition-colors" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"></path></svg>
            </button>

            <!-- Project Selector -->
            <button 
            @click="selectProject"
            class="group relative px-5 py-3 bg-white hover:bg-gray-50 border border-gray-200 rounded-xl shadow-sm hover:shadow-md transition-all duration-200 ease-out flex items-center gap-3 overflow-hidden"
            >
            <div class="flex flex-col items-start text-left max-w-[200px]">
                <span class="text-xs font-semibold text-gray-400 uppercase tracking-wider">Current Repo</span>
                <span class="font-medium text-gray-700 truncate w-full" :title="projectPath">{{ projectPath ? projectPath.split(/[\\/]/).pop() : 'Select...' }}</span>
            </div>
            <svg class="w-5 h-5 text-gray-400 group-hover:text-blue-600 transition-colors" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path></svg>
            </button>
        </div>
      </header>

      <main>
        <!-- Error Alert -->
        <transition enter-active-class="transition ease-out duration-300" enter-from-class="opacity-0 -translate-y-2" enter-to-class="opacity-100 translate-y-0" leave-active-class="transition ease-in duration-200" leave-from-class="opacity-100" leave-to-class="opacity-0">
          <div v-if="errorMsg" class="mb-8 p-4 bg-red-50 border border-red-100 text-red-700 rounded-xl flex items-start gap-3 shadow-sm">
            <svg class="w-5 h-5 mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>
            <div class="flex-1 text-sm font-medium">{{ errorMsg }}</div>
            <button @click="errorMsg = ''" class="text-red-400 hover:text-red-600"><svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path></svg></button>
          </div>
        </transition>

        <!-- Main Content -->
        <div v-if="projectPath">
            <div class="flex justify-between items-end mb-6">
              <div class="flex items-center gap-3">
                 <h2 class="text-2xl font-bold text-gray-800">Worktrees</h2>
                 <span class="bg-blue-100 text-blue-700 font-bold px-2.5 py-0.5 rounded-full text-xs">{{ visibleWorktrees.length }}</span>
              </div>
              <button 
                @click="showModal = true"
                :disabled="!globalRoot"
                :class="{'opacity-50 cursor-not-allowed': !globalRoot}"
                class="px-5 py-2.5 bg-blue-600 text-white rounded-xl shadow-lg shadow-blue-200 hover:bg-blue-700 hover:shadow-blue-300 transition-all active:scale-95 font-medium flex items-center gap-2"
                :title="!globalRoot ? 'Please select a Worktree Root directory first' : ''"
              >
                <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"></path></svg>
                New Worktree
              </button>
            </div>
            
            <div v-if="!globalRoot" class="mb-6 p-4 bg-orange-50 border border-orange-100 text-orange-800 rounded-xl text-sm flex items-center gap-3">
                 <svg class="w-5 h-5 text-orange-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path></svg>
                 <span>Please select a <strong>Worktree Root Directory</strong> (top right) where new folders will be created.</span>
            </div>

            <div v-if="loading && worktrees.length === 0" class="py-20 text-center text-gray-400">
               <div class="animate-spin w-8 h-8 border-2 border-gray-300 border-t-blue-600 rounded-full mx-auto mb-4"></div>
               <p>Loading worktrees...</p>
            </div>

            <div v-else class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-5">
              <div v-for="wt in visibleWorktrees" :key="wt.path" class="group bg-white rounded-2xl p-5 border border-gray-100 shadow-[0_2px_8px_rgba(0,0,0,0.04)] hover:shadow-[0_8px_24px_rgba(0,0,0,0.08)] hover:-translate-y-1 transition-all duration-300 md:col-span-1">
                 
                 <!-- Branch Badge -->
                 <div class="flex justify-between items-start mb-3">
                    <div class="flex-1 min-w-0 pr-3">
                       <h3 class="font-bold text-lg text-gray-800 truncate" :title="wt.branch">{{ wt.branch || 'Detached' }}</h3>
                    </div>
                    <div class="flex-shrink-0">
                       <span class="font-mono text-[10px] uppercase bg-gray-100 text-gray-500 px-2 py-1 rounded-md border border-gray-200">{{ wt.head_hash.substring(0, 7) }}</span>
                    </div>
                 </div>

                 <!-- Path Info -->
                 <div class="text-xs text-gray-400 uppercase tracking-wider font-semibold mb-1">Location</div>
                 <div class="text-sm text-gray-600 font-mono break-all bg-gray-50/50 p-2 rounded-lg border border-gray-100 mb-4" :title="wt.path">
                    {{ wt.path }}
                 </div>

                 <!-- Footer Actions -->
                 <div class="pt-4 border-t border-gray-50 flex justify-between items-center">
                    <button 
                       @click="openFolder(wt.path)"
                       class="text-sm font-medium text-gray-500 hover:text-blue-600 hover:bg-blue-50 px-3 py-1.5 rounded-lg transition-colors flex items-center gap-1.5"
                       title="Open in Explorer"
                    >
                       <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"></path></svg>
                       Open
                    </button>

                    <button 
                       @click="removeWorktree(wt.path, wt.branch)"
                       class="text-sm font-medium text-red-500 hover:text-red-700 hover:bg-red-50 px-3 py-1.5 rounded-lg transition-colors flex items-center gap-1.5 opacity-80 group-hover:opacity-100"
                    >
                       <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path></svg>
                       Remove
                    </button>
                 </div>
              </div>
            </div>
        </div>

        <!-- Empty Start View -->
        <div v-if="!projectPath" class="mt-10 py-24 bg-white/50 border-2 border-dashed border-gray-200 rounded-3xl text-center backdrop-blur-sm">
           <div class="w-20 h-20 bg-blue-50 text-blue-500 rounded-2xl flex items-center justify-center mx-auto mb-6 shadow-sm">
             <svg class="w-10 h-10" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M8 7v8a2 2 0 002 2h6M8 7V5a2 2 0 012-2h4.586a1 1 0 01.707.293l4.414 4.414a1 1 0 01.293.707V15a2 2 0 01-2 2h-2M8 7H6a2 2 0 00-2 2v10a2 2 0 002 2h8a2 2 0 002-2v-2"></path></svg>
           </div>
           <h3 class="text-2xl font-bold text-gray-900 mb-2">No Project Selected</h3>
           <p class="text-gray-500 mb-8 max-w-md mx-auto">Select a Git repository to start managing its worktrees.</p>
           <button 
             @click="selectProject"
             class="px-8 py-3 bg-gray-900 text-white font-medium rounded-xl hover:bg-black hover:scale-105 transition-all shadow-xl shadow-gray-200"
           >
             Open Repository
           </button>
        </div>
      </main>

      <!-- Create Modal -->
      <transition enter-active-class="transition duration-200 ease-out" enter-from-class="opacity-0 scale-95" enter-to-class="opacity-100 scale-100" leave-active-class="transition duration-150 ease-in" leave-from-class="opacity-100 scale-100" leave-to-class="opacity-0 scale-95">
        <div v-if="showModal" class="fixed inset-0 z-50 flex items-center justify-center p-4">
           <div class="absolute inset-0 bg-gray-900/30 backdrop-blur-sm" @click="showModal = false"></div>
           <div class="relative bg-white rounded-2xl shadow-2xl w-full max-w-md overflow-hidden">
              <div class="px-6 py-5 border-b border-gray-100 flex justify-between items-center bg-gray-50/50">
                 <h3 class="text-lg font-bold text-gray-900">Create Environment</h3>
                 <button @click="showModal = false" class="text-gray-400 hover:text-gray-600"><svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path></svg></button>
              </div>
              
              <div class="p-6 space-y-5">
                 <div>
                    <label class="block text-sm font-semibold text-gray-700 mb-1.5">New Branch Name</label>
                    <div class="relative">
                       <input v-model="newBranch" placeholder="feature/user-login" autofocus class="w-full pl-9 pr-3 py-2.5 bg-gray-50 border border-gray-200 rounded-xl focus:bg-white focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all font-medium text-gray-800" />
                       <svg class="w-4 h-4 text-gray-400 absolute left-3 top-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 20l4-16m2 16l4-16M6 9h14M4 15h14"></path></svg>
                    </div>
                 </div>

                 <div>
                    <label class="block text-sm font-semibold text-gray-700 mb-1.5">Target Location</label>
                    <div class="w-full px-4 py-3 bg-blue-50/50 border border-blue-100 rounded-xl text-sm text-gray-600 font-mono break-all">
                       <span v-if="newBranch" class="text-blue-700">{{ computedPreviewPath }}</span>
                       <span v-else class="text-gray-400 opacity-50">{{ globalRoot }}/... (Type branch name)</span>
                    </div>
                    <p class="text-xs text-gray-400 mt-1.5">Based on Global Root + ProjectName - BranchName</p>
                 </div>

                 <div>
                    <label class="block text-sm font-semibold text-gray-700 mb-1.5">Base Branch</label>
                    <div class="relative">
                      <input v-model="baseBranch" class="w-full pl-9 pr-3 py-2.5 bg-gray-50 border border-gray-200 rounded-xl focus:bg-white focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all font-medium text-gray-800" />
                      <svg class="w-4 h-4 text-gray-400 absolute left-3 top-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18"></path></svg>
                    </div>
                 </div>
              </div>

              <div class="px-6 py-4 bg-gray-50 flex justify-end gap-3 border-t border-gray-100">
                 <button @click="showModal = false" class="px-4 py-2 text-gray-600 hover:text-gray-900 hover:bg-gray-100 rounded-lg font-medium transition-colors">Cancel</button>
                 <button 
                    @click="createWorktree" 
                    :disabled="loading || !newBranch || !baseBranch"
                    class="px-5 py-2 bg-gradient-to-r from-blue-600 to-indigo-600 text-white rounded-lg shadow-md hover:shadow-lg hover:to-indigo-700 font-medium disabled:opacity-50 disabled:cursor-not-allowed transform active:scale-95 transition-all flex items-center gap-2"
                 >
                    <span v-if="loading" class="animate-spin h-4 w-4 border-2 border-white border-t-transparent rounded-full"></span>
                    <span>Create & Checkout</span>
                 </button>
              </div>
           </div>
        </div>
      </transition>

    </div>
  </div>
</template>

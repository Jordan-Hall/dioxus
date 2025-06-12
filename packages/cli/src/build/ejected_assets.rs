use std::path::{Path, PathBuf};

/// Utility functions to handle ejected assets
pub(crate) struct EjectedAssets;

impl EjectedAssets {
    /// Check if there are ejected assets in the project directory
    pub fn has_ejected_assets(project_dir: &Path) -> bool {
        let android_dir = project_dir.join("android");
        let ios_dir = project_dir.join("ios");
        
        android_dir.exists() || ios_dir.exists()
    }
    
    /// Get the path to the ejected Android assets directory if it exists
    pub fn android_assets_dir(project_dir: &Path) -> Option<PathBuf> {
        let android_dir = project_dir.join("android");
        if android_dir.exists() {
            Some(android_dir)
        } else {
            None
        }
    }
    
    /// Get the path to the ejected iOS assets directory if it exists
    pub fn ios_assets_dir(project_dir: &Path) -> Option<PathBuf> {
        let ios_dir = project_dir.join("ios");
        if ios_dir.exists() {
            Some(ios_dir)
        } else {
            None
        }
    }
    
    /// Get the path to the ejected assets directory for a specific platform if it exists
    pub fn platform_assets_dir(project_dir: &Path, platform: &str) -> Option<PathBuf> {
        match platform {
            "android" => Self::android_assets_dir(project_dir),
            "ios" => Self::ios_assets_dir(project_dir),
            _ => None,
        }
    }
}

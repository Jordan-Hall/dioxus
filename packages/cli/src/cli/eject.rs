use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;

use crate::error::Result;
use crate::StructuredOutput;

/// Eject Android and iOS assets to a local directory for customization
#[derive(Parser)]
pub struct Eject {
    /// The directory to eject assets to. If not specified, will use the current directory.
    #[clap(short, long)]
    pub output_dir: Option<PathBuf>,

    /// Eject Android assets
    #[clap(long, default_value = "true")]
    pub android: bool,

    /// Eject iOS assets
    #[clap(long, default_value = "true")]
    pub ios: bool,

    /// Force overwrite of existing files
    #[clap(short, long)]
    pub force: bool,
}

impl Eject {
    pub fn eject(&self) -> Result<StructuredOutput> {
        let output_dir = self.output_dir.clone().unwrap_or_else(|| PathBuf::from("."));
        
        // Create the output directory if it doesn't exist
        fs::create_dir_all(&output_dir)?;
        
        if self.android {
            self.eject_android_assets(&output_dir)?;
        }
        
        if self.ios {
            self.eject_ios_assets(&output_dir)?;
        }
        
        println!("Successfully ejected assets to {}", output_dir.display());
        Ok(StructuredOutput::Success)
    }
    
    fn eject_android_assets(&self, output_dir: &Path) -> Result<()> {
        let android_dir = output_dir.join("android");
        
        // Create android directory
        fs::create_dir_all(&android_dir)?;
        
        // Get the path to the embedded Android assets
        let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("android");
        
        // Copy the Android assets
        self.copy_directory_contents(&assets_dir, &android_dir)?;
        
        println!("Ejected Android assets to {}", android_dir.display());
        Ok(())
    }
    
    fn eject_ios_assets(&self, output_dir: &Path) -> Result<()> {
        let ios_dir = output_dir.join("ios");
        
        // Create ios directory
        fs::create_dir_all(&ios_dir)?;
        
        // Get the path to the embedded iOS assets
        let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("ios");
        
        // Copy the iOS assets
        self.copy_directory_contents(&assets_dir, &ios_dir)?;
        
        println!("Ejected iOS assets to {}", ios_dir.display());
        Ok(())
    }
    
    fn copy_directory_contents(&self, src: &Path, dst: &Path) -> Result<()> {
        if !src.exists() {
            return Err(anyhow::anyhow!("Source directory does not exist: {}", src.display()).into());
        }
        
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest_path = dst.join(path.file_name().unwrap());
            
            if dest_path.exists() && !self.force {
                println!("Skipping {} as it already exists (use --force to overwrite)", dest_path.display());
                continue;
            }
            
            if path.is_dir() {
                fs::create_dir_all(&dest_path)?;
                self.copy_directory_contents(&path, &dest_path)?;
            } else {
                fs::copy(&path, &dest_path)?;
            }
        }
        
        Ok(())
    }
}

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Result, Context};
use tracing::{info, warn, error};

use crate::recording_config::FileRotationSettings;

/// File rotation manager for recording output
pub struct FileRotationManager {
    settings: FileRotationSettings,
    base_path: PathBuf,
    current_file: Option<BufWriter<File>>,
    current_file_path: Option<PathBuf>,
    current_file_size: u64,
    file_counter: u32,
}

impl FileRotationManager {
    pub fn new(output_pattern: &str, settings: FileRotationSettings) -> Result<Self> {
        let base_path = Self::resolve_output_path(output_pattern)?;
        
        Ok(Self {
            settings,
            base_path,
            current_file: None,
            current_file_path: None,
            current_file_size: 0,
            file_counter: 0,
        })
    }
    
    /// Resolve output path pattern with timestamp substitution
    pub fn resolve_output_path(pattern: &str) -> Result<PathBuf> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get current timestamp")?
            .as_secs();
        
        let resolved = pattern.replace("{timestamp}", &timestamp.to_string());
        Ok(PathBuf::from(resolved))
    }
    
    /// Get current writer, creating new file if needed
    pub fn get_writer(&mut self) -> Result<&mut BufWriter<File>> {
        // Check if we need to rotate
        if self.should_rotate() {
            self.rotate()?;
        }
        
        // Create initial file if needed
        if self.current_file.is_none() {
            self.create_new_file()?;
        }
        
        Ok(self.current_file.as_mut().unwrap())
    }
    
    /// Write data and track size
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        let writer = self.get_writer()?;
        writer.write_all(data)?;
        self.current_file_size += data.len() as u64;
        Ok(())
    }
    
    /// Flush current writer
    pub fn flush(&mut self) -> Result<()> {
        if let Some(writer) = &mut self.current_file {
            writer.flush()?;
        }
        Ok(())
    }
    
    /// Check if file rotation is needed
    fn should_rotate(&self) -> bool {
        if !self.settings.enabled {
            return false;
        }
        
        if self.current_file.is_none() {
            return false;
        }
        
        let max_size_bytes = self.settings.max_size_mb * 1024 * 1024;
        self.current_file_size >= max_size_bytes
    }
    
    /// Rotate to a new file
    fn rotate(&mut self) -> Result<()> {
        info!("Rotating log file, current size: {} MB", self.current_file_size / 1024 / 1024);
        
        // Close current file
        if let Some(mut writer) = self.current_file.take() {
            writer.flush()?;
        }
        
        // Compress previous file if enabled
        if self.settings.compress_rotated {
            if let Some(path) = &self.current_file_path {
                self.compress_file(path)?;
            }
        }
        
        // Clean up old files
        self.cleanup_old_files()?;
        
        // Reset for new file
        self.current_file_size = 0;
        self.current_file_path = None;
        
        Ok(())
    }
    
    /// Create a new output file
    fn create_new_file(&mut self) -> Result<()> {
        let file_path = if self.file_counter == 0 {
            self.base_path.clone()
        } else {
            let stem = self.base_path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("recording");
            let extension = self.base_path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("pb");
            
            let parent = self.base_path.parent().unwrap_or(Path::new("."));
            parent.join(format!("{}_{:03}.{}", stem, self.file_counter, extension))
        };
        
        info!("Creating new recording file: {}", file_path.display());
        
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .with_context(|| format!("Failed to create file: {}", file_path.display()))?;
        
        self.current_file = Some(BufWriter::new(file));
        self.current_file_path = Some(file_path);
        self.file_counter += 1;
        
        Ok(())
    }
    
    /// Compress a file using gzip
    fn compress_file(&self, path: &Path) -> Result<()> {
        use std::process::Command;
        
        info!("Compressing file: {}", path.display());
        
        let output = Command::new("gzip")
            .arg(path)
            .output()
            .context("Failed to execute gzip command")?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to compress file {}: {}", path.display(), stderr);
        } else {
            info!("Successfully compressed file: {}.gz", path.display());
        }
        
        Ok(())
    }
    
    /// Clean up old files beyond the retention limit
    fn cleanup_old_files(&self) -> Result<()> {
        if self.settings.max_files == 0 {
            return Ok(());
        }
        
        let parent_dir = self.base_path.parent().unwrap_or(Path::new("."));
        let base_name = self.base_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("recording");
        
        // Find all related files
        let mut files = Vec::new();
        
        if let Ok(entries) = std::fs::read_dir(parent_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(base_name) && (name.ends_with(".pb") || name.ends_with(".pb.gz")) {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                files.push((path, modified));
                            }
                        }
                    }
                }
            }
        }
        
        // Sort by modification time (oldest first)
        files.sort_by_key(|(_, time)| *time);
        
        // Remove excess files
        let files_to_remove = files.len().saturating_sub(self.settings.max_files as usize);
        for (path, _) in files.iter().take(files_to_remove) {
            info!("Removing old recording file: {}", path.display());
            if let Err(e) = std::fs::remove_file(path) {
                warn!("Failed to remove old file {}: {}", path.display(), e);
            }
        }
        
        Ok(())
    }
    
    /// Get current file path
    pub fn current_file_path(&self) -> Option<&Path> {
        self.current_file_path.as_deref()
    }
    
    /// Get current file size in bytes
    pub fn current_file_size(&self) -> u64 {
        self.current_file_size
    }
    
    /// Get file counter
    pub fn file_counter(&self) -> u32 {
        self.file_counter
    }
}

impl Drop for FileRotationManager {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            error!("Failed to flush file on drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_file_rotation_manager_creation() {
        let settings = FileRotationSettings {
            enabled: true,
            max_size_mb: 1,
            max_files: 5,
            compress_rotated: false,
        };
        
        let manager = FileRotationManager::new("test_{timestamp}.pb", settings);
        assert!(manager.is_ok());
    }
    
    #[test]
    fn test_timestamp_substitution() {
        let pattern = "recording_{timestamp}.pb";
        let resolved = FileRotationManager::resolve_output_path(pattern).unwrap();
        let path_str = resolved.to_string_lossy();
        
        assert!(path_str.starts_with("recording_"));
        assert!(path_str.ends_with(".pb"));
        assert!(!path_str.contains("{timestamp}"));
    }
    
    #[test]
    fn test_file_creation_and_writing() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let output_path = temp_dir.path().join("test.pb");
        
        let settings = FileRotationSettings {
            enabled: false,
            max_size_mb: 1,
            max_files: 5,
            compress_rotated: false,
        };
        
        let mut manager = FileRotationManager::new(
            output_path.to_str().unwrap(),
            settings
        )?;
        
        let test_data = b"test data";
        manager.write(test_data)?;
        manager.flush()?;
        
        assert_eq!(manager.current_file_size(), test_data.len() as u64);
        
        Ok(())
    }
}
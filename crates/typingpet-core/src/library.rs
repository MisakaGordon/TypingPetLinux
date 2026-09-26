//! 图片集：文件夹导入规则、内置/自定义图库、激活集切换。
//!
//! 移植自 macOS 版 `PetImageLibrary.swift`，平台相关部分（`NSImage` 校验、`Bundle.main`）
//! 换成 `image` crate 与显式资源目录。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "gif", "tif", "tiff", "heic", "heif", "webp",
];

pub const BUILT_IN_SET_ID: &str = "00000000-0000-0000-0000-000000000001";
pub const DEFAULT_IDLE_NAME: &str = "pet-idle.png";
pub const DEFAULT_REACTION_NAMES: &[&str] = &[
    "pet-left.png",
    "pet-right.png",
    "pet-question.png",
    "pet-exclamation.png",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderImageSelection {
    pub idle_url: Option<PathBuf>,
    pub reaction_urls: Vec<PathBuf>,
}

impl FolderImageSelection {
    /// 规则：`idle.*` / `pet-idle.*` 作为待机图；其余支持格式作为反应图；
    /// 无待机图时取排序后的第一张；排序使用大小写不敏感的文件名比较。
    pub fn make(urls: &[PathBuf]) -> Self {
        let mut images: Vec<PathBuf> = urls
            .iter()
            .filter(|url| {
                url.extension()
                    .and_then(|value| value.to_str())
                    .map(|value| SUPPORTED_EXTENSIONS.contains(&value.to_lowercase().as_str()))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        images.sort_by(|left, right| natural_key(left).cmp(&natural_key(right)));

        let idle_url = images
            .iter()
            .find(|url| {
                let stem = file_stem_lowercase(url);
                stem == "idle" || stem == "pet-idle"
            })
            .cloned();

        let reaction_urls = images
            .iter()
            .filter(|url| match &idle_url {
                Some(idle) => *url != idle,
                None => true,
            })
            .cloned()
            .collect();

        Self {
            idle_url,
            reaction_urls,
        }
    }
}

fn file_stem_lowercase(url: &Path) -> String {
    url.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// 大小写不敏感的文件名排序键；数字段补零以获得接近"自然排序"的结果。
fn natural_key(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mut key = String::with_capacity(name.len());
    let mut digits = String::new();
    for character in name.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
        } else {
            if !digits.is_empty() {
                key.push_str(&format!("{:0>10}", digits));
                digits.clear();
            }
            key.push(character);
        }
    }
    if !digits.is_empty() {
        key.push_str(&format!("{:0>10}", digits));
    }
    key
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PetImageSet {
    pub id: String,
    pub name: String,
    pub storage_folder: Option<String>,
    pub idle_file_name: String,
    pub reaction_file_names: Vec<String>,
    pub is_built_in: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderImportResult {
    pub image_set: PetImageSet,
    pub changed_idle: bool,
    pub reaction_count: usize,
}

#[derive(Debug)]
pub enum ImageLibraryError {
    Io(std::io::Error),
    NoImagesInFolder,
    NoReactionImages,
    ImageSetNotFound,
    CannotDeleteBuiltInSet,
    UnsupportedFormat(String),
    UnreadableImage(String),
}

impl std::fmt::Display for ImageLibraryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::NoImagesInFolder => write!(formatter, "no supported images found in folder"),
            Self::NoReactionImages => write!(formatter, "select at least one reaction image"),
            Self::ImageSetNotFound => write!(formatter, "image set not found"),
            Self::CannotDeleteBuiltInSet => write!(formatter, "built-in set cannot be deleted"),
            Self::UnsupportedFormat(name) => write!(formatter, "unsupported image format: {name}"),
            Self::UnreadableImage(name) => write!(formatter, "cannot read image: {name}"),
        }
    }
}

impl std::error::Error for ImageLibraryError {}

impl From<std::io::Error> for ImageLibraryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// 图库：内置集 + 自定义集，自定义集元数据以 JSON 落盘。
#[derive(Debug)]
pub struct ImageLibrary {
    images_directory: PathBuf,
    sets_directory: PathBuf,
    gallery_path: PathBuf,
    resources_directory: PathBuf,
    custom_sets: Vec<PetImageSet>,
    active_set_id: String,
}

impl ImageLibrary {
    pub fn new(
        images_directory: impl Into<PathBuf>,
        resources_directory: impl Into<PathBuf>,
    ) -> Self {
        let images_directory = images_directory.into();
        let sets_directory = images_directory.join("Sets");
        let gallery_path = images_directory.join("gallery.json");
        let resources_directory = resources_directory.into();

        let custom_sets = fs::read_to_string(&gallery_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<PetImageSet>>(&text).ok())
            .map(|sets| sets.into_iter().filter(|set| !set.is_built_in).collect())
            .unwrap_or_default();

        let active_set_id = fs::read_to_string(images_directory.join("active-set.txt"))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| BUILT_IN_SET_ID.to_string());

        let mut library = Self {
            images_directory,
            sets_directory,
            gallery_path,
            resources_directory,
            custom_sets,
            active_set_id,
        };
        library.repair_active_selection();
        library
    }

    pub fn built_in_set() -> PetImageSet {
        PetImageSet {
            id: BUILT_IN_SET_ID.to_string(),
            name: "built-in".to_string(),
            storage_folder: None,
            idle_file_name: DEFAULT_IDLE_NAME.to_string(),
            reaction_file_names: DEFAULT_REACTION_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            is_built_in: true,
        }
    }

    pub fn image_sets(&self) -> Vec<PetImageSet> {
        let mut sets = vec![Self::built_in_set()];
        sets.extend(self.custom_sets.iter().cloned());
        sets
    }

    pub fn active_set_id(&self) -> &str {
        &self.active_set_id
    }

    pub fn active_set(&self) -> PetImageSet {
        self.image_sets()
            .into_iter()
            .find(|set| set.id == self.active_set_id)
            .unwrap_or_else(Self::built_in_set)
    }

    pub fn uses_custom_images(&self) -> bool {
        self.active_set_id != BUILT_IN_SET_ID
    }

    pub fn idle_url(&self) -> Option<PathBuf> {
        self.idle_url_for(&self.active_set())
    }

    pub fn reaction_urls(&self) -> Vec<PathBuf> {
        self.reaction_urls_for(&self.active_set())
    }

    pub fn idle_url_for(&self, set: &PetImageSet) -> Option<PathBuf> {
        self.resolved_url(&set.idle_file_name, set)
    }

    pub fn reaction_urls_for(&self, set: &PetImageSet) -> Vec<PathBuf> {
        set.reaction_file_names
            .iter()
            .filter_map(|name| self.resolved_url(name, set))
            .collect()
    }

    pub fn activate_set(&mut self, id: &str) -> Result<(), ImageLibraryError> {
        if !self.image_sets().iter().any(|set| set.id == id) {
            return Err(ImageLibraryError::ImageSetNotFound);
        }
        self.active_set_id = id.to_string();
        self.persist_active();
        Ok(())
    }

    pub fn add_set(&mut self, folder: &Path, name: Option<&str>) -> Result<PetImageSet, ImageLibraryError> {
        let mut contents = Vec::new();
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && !entry.file_name().to_string_lossy().starts_with('.') {
                contents.push(path);
            }
        }

        let selection = FolderImageSelection::make(&contents);
        let mut all_images: Vec<PathBuf> = selection.idle_url.iter().cloned().collect();
        all_images.extend(selection.reaction_urls.iter().cloned());
        if all_images.is_empty() {
            return Err(ImageLibraryError::NoImagesInFolder);
        }

        let idle_source = selection.idle_url.clone().unwrap_or_else(|| all_images[0].clone());
        let mut reaction_sources: Vec<PathBuf> = selection
            .reaction_urls
            .iter()
            .filter(|url| **url != idle_source)
            .cloned()
            .collect();
        if reaction_sources.is_empty() {
            reaction_sources.push(idle_source.clone());
        }

        let id = crate::unique_id();
        let destination = self.sets_directory.join(&id);
        fs::create_dir_all(&destination)?;

        let idle_name = copy_image(&idle_source, &destination)?;
        let reaction_names = reaction_sources
            .iter()
            .map(|source| copy_image(source, &destination))
            .collect::<Result<Vec<_>, _>>()?;

        let display_name = name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                folder
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("set")
                    .to_string()
            });

        let set = PetImageSet {
            id: id.clone(),
            name: display_name,
            storage_folder: Some(id.clone()),
            idle_file_name: idle_name,
            reaction_file_names: reaction_names,
            is_built_in: false,
        };
        self.custom_sets.push(set.clone());
        self.persist_gallery()?;
        self.active_set_id = id;
        self.persist_active();
        Ok(set)
    }

    pub fn import_folder(&mut self, folder: &Path) -> Result<FolderImportResult, ImageLibraryError> {
        let set = self.add_set(folder, None)?;
        Ok(FolderImportResult {
            changed_idle: true,
            reaction_count: set.reaction_file_names.len(),
            image_set: set,
        })
    }

    pub fn delete_set(&mut self, id: &str) -> Result<(), ImageLibraryError> {
        if id == BUILT_IN_SET_ID {
            return Err(ImageLibraryError::CannotDeleteBuiltInSet);
        }
        let before = self.custom_sets.len();
        self.custom_sets.retain(|set| set.id != id);
        if self.custom_sets.len() == before {
            return Err(ImageLibraryError::ImageSetNotFound);
        }
        if self.active_set_id == id {
            self.active_set_id = BUILT_IN_SET_ID.to_string();
            self.persist_active();
        }
        self.persist_gallery()
    }

    pub fn rename_set(&mut self, id: &str, name: &str) -> Result<(), ImageLibraryError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ImageLibraryError::ImageSetNotFound);
        }
        let set = self
            .custom_sets
            .iter_mut()
            .find(|set| set.id == id)
            .ok_or(ImageLibraryError::ImageSetNotFound)?;
        set.name = trimmed.to_string();
        self.persist_gallery()
    }

    pub fn reset_to_defaults(&mut self) {
        self.active_set_id = BUILT_IN_SET_ID.to_string();
        self.persist_active();
    }

    pub fn images_directory(&self) -> &Path {
        &self.images_directory
    }

    pub fn resources_directory(&self) -> &Path {
        &self.resources_directory
    }

    fn resolved_url(&self, file_name: &str, set: &PetImageSet) -> Option<PathBuf> {
        if set.is_built_in {
            let path = self.resources_directory.join(file_name);
            return path.is_file().then_some(path);
        }
        let base = match &set.storage_folder {
            Some(folder) => self.sets_directory.join(folder),
            None => self.images_directory.clone(),
        };
        let path = base.join(file_name);
        if path.is_file() {
            return Some(path);
        }
        // 自定义集中引用了内置素材时回退到资源目录
        if DEFAULT_REACTION_NAMES.contains(&file_name) || file_name == DEFAULT_IDLE_NAME {
            let fallback = self.resources_directory.join(file_name);
            return fallback.is_file().then_some(fallback);
        }
        None
    }

    fn persist_gallery(&self) -> Result<(), ImageLibraryError> {
        fs::create_dir_all(&self.images_directory)?;
        let text = serde_json::to_string_pretty(&self.custom_sets)
            .map_err(|error| ImageLibraryError::Io(std::io::Error::new(std::io::ErrorKind::Other, error)))?;
        fs::write(&self.gallery_path, text)?;
        Ok(())
    }

    fn persist_active(&self) {
        fs::create_dir_all(&self.images_directory).ok();
        fs::write(self.images_directory.join("active-set.txt"), &self.active_set_id).ok();
    }

    fn repair_active_selection(&mut self) {
        if !self.image_sets().iter().any(|set| set.id == self.active_set_id) {
            self.active_set_id = BUILT_IN_SET_ID.to_string();
        }
    }
}

fn copy_image(source: &Path, destination_directory: &Path) -> Result<String, ImageLibraryError> {
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(ImageLibraryError::UnsupportedFormat(
            source
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default(),
        ));
    }
    if image::image_dimensions(source).is_err() {
        return Err(ImageLibraryError::UnreadableImage(
            source
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default(),
        ));
    }

    fs::create_dir_all(destination_directory)?;
    let safe: String = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image")
        .chars()
        .map(|character| match character {
            '/' | ':' => '-',
            other => other,
        })
        .collect();
    let name = format!("{}-{}.{}", crate::unique_id(), safe, extension);
    fs::copy(source, destination_directory.join(&name))?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1x1 的最小合法 PNG，用于图片校验路径。
    const FAKE_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    // 移植自 Tests/TypingPetTests/FolderImageSelectionTests.swift
    #[test]
    fn recognizes_pet_idle_and_sorts_reaction_images() {
        let urls = vec![
            PathBuf::from("/set/z-last.webp"),
            PathBuf::from("/set/pet-idle.png"),
            PathBuf::from("/set/a-first.gif"),
            PathBuf::from("/set/readme.txt"),
        ];

        let selection = FolderImageSelection::make(&urls);

        assert_eq!(
            selection
                .idle_url
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_string()),
            Some("pet-idle.png".to_string())
        );
        let reactions: Vec<String> = selection
            .reaction_urls
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(reactions, vec!["a-first.gif", "z-last.webp"]);
    }

    #[test]
    fn no_idle_name_means_no_idle_and_all_images_are_reactions() {
        let urls = vec![PathBuf::from("/set/b.png"), PathBuf::from("/set/a.png")];
        let selection = FolderImageSelection::make(&urls);
        // `make` 只负责识别 idle.*/pet-idle.*；"无待机图时取第一张"由 `add_set` 兜底
        assert!(selection.idle_url.is_none());
        let reactions: Vec<String> = selection
            .reaction_urls
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(reactions, vec!["a.png", "b.png"]);
    }

    #[test]
    fn add_set_falls_back_to_first_sorted_image_as_idle() {
        let root = std::env::temp_dir().join(format!("typingpet-lib-{}", crate::unique_id()));
        let folder = root.join("set");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("b.png"), FAKE_PNG).expect("write b");
        fs::write(folder.join("a.png"), FAKE_PNG).expect("write a");

        let mut library = ImageLibrary::new(root.join("images"), root.join("builtin"));
        let set = library.add_set(&folder, None).expect("add set");

        assert!(set.idle_file_name.ends_with(".png"));
        assert_eq!(set.reaction_file_names.len(), 1);
        let idle_path = library.idle_url().expect("idle url");
        assert!(idle_path.is_file());
        // 兜底选的待机图应是排序第一张（a.png），反应图只有 b.png
        assert_eq!(
            idle_path.file_name().unwrap().to_string_lossy().contains("a"),
            true
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn built_in_set_has_expected_defaults() {
        let set = ImageLibrary::built_in_set();
        assert!(set.is_built_in);
        assert_eq!(set.idle_file_name, DEFAULT_IDLE_NAME);
        assert_eq!(set.reaction_file_names.len(), DEFAULT_REACTION_NAMES.len());
    }
}

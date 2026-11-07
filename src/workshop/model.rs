//! Workshop data types. Field names match the JSON produced by the original (Newtonsoft, PascalCase),
//! so library / config files use the same format as the C# launcher.

use serde::{Deserialize, Deserializer, Serialize};

/// Treats JSON `null` as the type's default.
fn nd<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct WorkshopFile {
    #[serde(default, deserialize_with = "nd")]
    pub guid: String,
    #[serde(default, deserialize_with = "nd")]
    pub name: String,
    #[serde(default, deserialize_with = "nd")]
    pub url: String,
    #[serde(default, deserialize_with = "nd")]
    pub md5: String,
    #[serde(default, deserialize_with = "nd")]
    pub language: String,
    #[serde(default, deserialize_with = "nd")]
    pub size: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Entry {
    #[serde(default, deserialize_with = "nd")]
    pub guid: String,
    #[serde(default, deserialize_with = "nd")]
    pub name: String,
    #[serde(default, deserialize_with = "nd")]
    pub version: String,
    #[serde(default, deserialize_with = "nd")]
    pub description: String,
    #[serde(default, deserialize_with = "nd")]
    pub changelog: String,
    #[serde(default, deserialize_with = "nd")]
    pub social_links: Vec<String>,
    #[serde(default, deserialize_with = "nd")]
    pub language: String,
    #[serde(default, deserialize_with = "nd")]
    pub artwork_url: String,
    #[serde(default, deserialize_with = "nd")]
    pub screenshot_urls: Vec<String>,
    #[serde(default, deserialize_with = "nd")]
    pub author: String,
    #[serde(default, deserialize_with = "nd")]
    pub owner: String,
    #[serde(default, deserialize_with = "nd")]
    pub game: i32,
    #[serde(default, deserialize_with = "nd")]
    pub r#type: i32,
    #[serde(default, deserialize_with = "nd")]
    pub creation_time: i64,
    #[serde(default, deserialize_with = "nd")]
    pub files: Vec<WorkshopFile>,
    /// Kept opaque so that maps/factions round-trip losslessly.
    #[serde(default, deserialize_with = "nd")]
    pub maps: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "nd")]
    pub factions: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "nd")]
    pub dependencies: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Metadata {
    #[serde(default, deserialize_with = "nd")]
    pub downloads: i64,
    #[serde(default, deserialize_with = "nd")]
    pub versions: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Preview {
    #[serde(default, deserialize_with = "nd")]
    pub guid: String,
    #[serde(default, deserialize_with = "nd")]
    pub name: String,
    #[serde(default, deserialize_with = "nd")]
    pub version: String,
    #[serde(default, deserialize_with = "nd")]
    pub description: String,
    #[serde(default, deserialize_with = "nd")]
    pub changelog: String,
    #[serde(default, deserialize_with = "nd")]
    pub social_links: Vec<String>,
    #[serde(default, deserialize_with = "nd")]
    pub language: String,
    #[serde(default, deserialize_with = "nd")]
    pub artwork_url: String,
    #[serde(default, deserialize_with = "nd")]
    pub screenshot_urls: Vec<String>,
    #[serde(default, deserialize_with = "nd")]
    pub author: String,
    #[serde(default, deserialize_with = "nd")]
    pub owner: String,
    #[serde(default, deserialize_with = "nd")]
    pub game: i32,
    #[serde(default, deserialize_with = "nd")]
    pub r#type: i32,
    #[serde(default, deserialize_with = "nd")]
    pub size: i64,
    #[serde(default, deserialize_with = "nd")]
    pub creation_time: i64,
    #[serde(default, deserialize_with = "nd")]
    pub metadata: Metadata,
}

pub fn game_name(game: i32) -> String {
    match game {
        -1 => "Unknown".into(),
        2 => "RotWK".into(),
        g => format!("BFME{}", g + 1),
    }
}

impl Entry {
    pub fn is_base_game(&self) -> bool {
        self.guid.starts_with("original-") || self.guid.starts_with("exp-original-")
    }
    pub fn game_name(&self) -> String {
        game_name(self.game)
    }
    pub fn full_name(&self) -> String {
        format!("{} ({})", self.name, self.version)
    }
    pub fn game_idx(&self) -> usize {
        self.game.clamp(0, 2) as usize
    }
    pub fn preview(&self) -> Preview {
        Preview {
            guid: self.guid.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            changelog: self.changelog.clone(),
            social_links: self.social_links.clone(),
            language: self.language.clone(),
            artwork_url: self.artwork_url.clone(),
            screenshot_urls: self.screenshot_urls.clone(),
            author: self.author.clone(),
            owner: self.owner.clone(),
            game: self.game,
            r#type: self.r#type,
            size: self.files.iter().map(|f| f.size).sum(),
            creation_time: self.creation_time,
            metadata: Metadata::default(),
        }
    }

    /// `BfmeWorkshopEntry.GetDestinationDirectory`
    pub fn destination_directory(&self, vr: &super::VirtualRegistry) -> String {
        let g = &vr[self.game_idx()];
        match self.r#type {
            1 => super::mod_directory(&g.game_dir, &self.name, &self.guid),
            3 => crate::util::combine(&[&crate::util::app_data(), &g.data_dir, "Maps", "Workshop"]),
            _ => g.game_dir.clone(),
        }
    }
}

impl Preview {
    pub fn game_name(&self) -> String {
        game_name(self.game)
    }
}

pub fn type_key(t: i32) -> &'static str {
    match t {
        0 => "LibraryTilePatchType",
        1 => "LibraryTileModType",
        2 => "LibraryTileEnhancementType",
        3 => "LibraryTileMapPackType",
        _ => "LibraryTileSnapshotType",
    }
}

/// Patches, mods and snapshots are "synced"; enhancements and map packs are toggled.
pub fn is_patch_like(t: i32) -> bool {
    t == 0 || t == 1 || t == 4
}

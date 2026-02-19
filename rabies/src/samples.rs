use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SampleMark {
    pub id: usize,
    pub sample_path: String,
    pub sample_name: String,
    pub position: f32,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct MarkerRelation {
    pub from_marker: usize,
    pub to_markers: Vec<usize>,
}

// ✅ NEW: Custom region structure
#[derive(Debug, Clone)]
pub struct CustomRegion {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub name: String,  // Auto-generated or user-provided
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackMode {
    PlayToEnd,
    PlayToNextMarker,
    CustomRegion { region_id: usize },  // References a region by ID
}

pub struct SamplesManager {
    marks: RwLock<Vec<SampleMark>>,
    next_id: RwLock<usize>,
    relations: RwLock<HashMap<usize, Vec<usize>>>,
    pub playback_mode: RwLock<PlaybackMode>,
    
    // ✅ NEW: Region management
    regions: RwLock<Vec<CustomRegion>>,
    next_region_id: RwLock<usize>,
}

impl SamplesManager {
    pub fn new() -> Self {
        Self {
            marks: RwLock::new(Vec::new()),
            next_id: RwLock::new(1),
            relations: RwLock::new(HashMap::new()),
            playback_mode: RwLock::new(PlaybackMode::PlayToEnd),
            regions: RwLock::new(Vec::new()),
            next_region_id: RwLock::new(1),
        }
    }

    pub fn mark_current_position(&self, sample_path: &str, sample_name: &str, position: f32) {
        let mut next_id = self.next_id.write();
        let id = *next_id;
        *next_id += 1;
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        let mark = SampleMark {
            id,
            sample_path: sample_path.to_string(),
            sample_name: sample_name.to_string(),
            position,
            timestamp,
        };
        
        self.marks.write().push(mark);
    }

    pub fn get_marks(&self) -> Vec<SampleMark> {
        self.marks.read().clone()
    }

    pub fn get_marks_for_sample(&self, sample_name: &str) -> Vec<SampleMark> {
        self.marks
            .read()
            .iter()
            .filter(|m| m.sample_name == sample_name)
            .cloned()
            .collect()
    }

    pub fn clear_marks(&self) {
        self.marks.write().clear();
        self.relations.write().clear();
        // Also clear regions when clearing marks
        self.regions.write().clear();
        *self.playback_mode.write() = PlaybackMode::PlayToEnd;
    }

    pub fn update_mark_position(&self, index: usize, new_position: f32) {
        if let Some(mark) = self.marks.write().get_mut(index) {
            mark.position = new_position.clamp(0.0, 1.0);
        }
    }

    pub fn find_mark_near(&self, sample_name: &str, position: f32, threshold: f32) -> Option<usize> {
        let marks = self.marks.read();
        marks.iter().enumerate().find(|(_, mark)| {
            mark.sample_name == sample_name && (mark.position - position).abs() < threshold
        }).map(|(idx, _)| idx)
    }

    pub fn add_relation(&self, from_marker: usize, to_markers: Vec<usize>) {
        self.relations.write().insert(from_marker, to_markers);
    }

    pub fn get_end_markers_for(&self, from_marker: usize) -> Vec<usize> {
        self.relations
            .read()
            .get(&from_marker)
            .cloned()
            .unwrap_or_default()
    }

    // ✅ NEW: Create a custom region
    pub fn create_region(&self, from: usize, to: usize) -> usize {
        let mut next_id = self.next_region_id.write();
        let id = *next_id;
        *next_id += 1;
        
        let region = CustomRegion {
            id,
            from,
            to,
            name: format!("Region {} → {}", from, to),
        };
        
        self.regions.write().push(region);
        id
    }

    // ✅ NEW: Get all regions
    pub fn get_regions(&self) -> Vec<CustomRegion> {
        self.regions.read().clone()
    }

    // ✅ NEW: Get a specific region by ID
    pub fn get_region_by_id(&self, id: usize) -> Option<CustomRegion> {
        self.regions.read().iter().find(|r| r.id == id).cloned()
    }

    // ✅ NEW: Delete a region
    pub fn delete_region(&self, id: usize) {
        self.regions.write().retain(|r| r.id != id);
        
        // If the deleted region was active, switch to PlayToEnd
        if let PlaybackMode::CustomRegion { region_id } = *self.playback_mode.read() {
            if region_id == id {
                *self.playback_mode.write() = PlaybackMode::PlayToEnd;
            }
        }
    }

    // ✅ NEW: Rename a region
    pub fn rename_region(&self, id: usize, new_name: String) {
        if let Some(region) = self.regions.write().iter_mut().find(|r| r.id == id) {
            region.name = new_name;
        }
    }

    pub fn get_playback_target(&self, current_pos: f32, sample_name: &str) -> Option<f32> {
        let mode = self.playback_mode.read().clone();
        let marks = self.get_marks_for_sample(sample_name);
        
        const MIN_DISTANCE: f32 = 0.005;
        
        match mode {
            PlaybackMode::PlayToEnd => None,
            PlaybackMode::PlayToNextMarker => {
                marks
                    .iter()
                    .filter(|m| m.position > current_pos + MIN_DISTANCE)
                    .min_by(|a, b| a.position.partial_cmp(&b.position).unwrap())
                    .map(|m| m.position)
            }
            PlaybackMode::CustomRegion { region_id } => {
                // ✅ UPDATED: Look up the region by ID
                if let Some(region) = self.get_region_by_id(region_id) {
                    marks.iter().find(|m| m.id == region.to).map(|m| m.position)
                } else {
                    None
                }
            }
        }
    }

    pub fn should_stop_at(&self, current_pos: f32, sample_name: &str) -> bool {
        if let Some(target) = self.get_playback_target(current_pos, sample_name) {
            current_pos >= target
        } else {
            false
        }
    }

    pub fn set_playback_mode(&self, mode: PlaybackMode) {
        *self.playback_mode.write() = mode;
    }

    pub fn get_playback_mode(&self) -> PlaybackMode {
        self.playback_mode.read().clone()
    }

    pub fn delete_mark(&self, index: usize) {
        let mut marks = self.marks.write();
        if index < marks.len() {
            let removed_id = marks.remove(index).id;
            
            // Remove relations
            let mut relations = self.relations.write();
            relations.remove(&removed_id);
            for (_, to_markers) in relations.iter_mut() {
                to_markers.retain(|&id| id != removed_id);
            }
            
            // ✅ NEW: Remove regions that reference this marker
            drop(relations);
            drop(marks);
            
            let mut regions = self.regions.write();
            regions.retain(|r| r.from != removed_id && r.to != removed_id);
        }
    }

    pub fn get_mark_by_id(&self, id: usize) -> Option<SampleMark> {
        self.marks.read().iter().find(|m| m.id == id).cloned()
    }
}
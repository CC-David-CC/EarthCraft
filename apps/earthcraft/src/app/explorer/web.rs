// SPDX-License-Identifier: MIT
//! The Explorer's pickers in Playground's browser build (patches/google-earth.yaml in github.com/daijro/openplayground):
//! Playground's dialogs answer after the frame (app/web.rs), and their answer lands here, as the desktop's dialog
//! answer does in explorer.rs.
use earthcraft_data::files::FileJobs;
use std::path::PathBuf;

impl super::Explorer {
    /// An image picked for the planet's texture, or for its sky.
    pub(in crate::app) fn picked_image(&mut self, path: String, sky: bool) {
        if sky {
            self.sky_path = path;
            self.sky_bytes = None;
        } else {
            self.image_path = path;
            self.image_bytes = None;
        }
    }

    /// Where Save planet… put the planet.
    pub(in crate::app) fn picked_save(&mut self, path: PathBuf, jobs: &FileJobs) {
        self.request_save(path, jobs);
    }
}

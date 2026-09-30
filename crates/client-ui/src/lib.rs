//! The signalbox client's screens (spec D1 §3), drawn with egui over
//! `client_core::App`. No windowing, no GPU and no browser here: the web
//! shell (`client-web`, D2's desktop shell later) runs `UiApp::ui` in its
//! frame loop. The diagram's scene, camera, hit-testing and drawing are
//! pure and tested headless.

pub mod camera;
pub mod hit;
pub mod paint;
pub mod scene;

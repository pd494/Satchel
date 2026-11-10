mod image;
mod link;
mod video;

pub use image::Image;
pub use link::Link;
use mime_guess::Mime;
use serde_json::{json, Value};
pub use video::Video;

pub trait Media {
    fn get_type(&self) -> &'static str;

    fn content(&self) -> String;

    fn title(&self) -> Option<&str> {
        None
    }

    fn description(&self) -> Option<&str> {
        None
    }

    fn data(&self) -> Value {
        json!({})
    }
}

/// Reads the attachment id and file type from a Linq file URL,
/// `https://cdn.linqapp.com/attachments/partners/<partner>/<id>/<filename>`.
fn linq_file(url: &str) -> Option<(String, Mime)> {
    let path = url.strip_prefix("https://cdn.linqapp.com/attachments/")?;
    let (folder, filename) = path.rsplit_once('/')?;
    let (_, id) = folder.rsplit_once('/')?;
    Some((id.to_owned(), mime_guess::from_path(filename).first()?))
}

#[cfg(test)]
mod tests {
    use super::{Image, Link, Video};

    const CDN: &str = "https://cdn.linqapp.com/attachments/partners/p1";

    #[test]
    fn reads_photos_and_videos_from_linq_urls_in_text() {
        let photo_id = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
        let text = format!("look at this\n{CDN}/{photo_id}/IMG_0001.HEIC\n{CDN}/9b1c/clip.MOV");
        let (_, urls) = Link::split(&format!("Here it is: {text}."));
        assert_eq!(urls.len(), 2);

        let image = Image::parse(&urls[0]).expect("photo is an image");
        assert_eq!(image.id, photo_id);
        assert!(Video::parse(&urls[0]).is_none());

        assert_eq!(Video::parse(&urls[1]).expect("clip is a video").id, "9b1c");
        assert!(Image::parse(&urls[1]).is_none());
    }

    #[test]
    fn ignores_other_files_and_sites() {
        assert!(Image::parse(&format!("{CDN}/7d4e/menu.pdf")).is_none());
        assert!(Video::parse(&format!("{CDN}/7d4e/menu.pdf")).is_none());
        assert!(Image::parse("https://example.com/attachments/partners/p1/1/a.jpg").is_none());
    }
}

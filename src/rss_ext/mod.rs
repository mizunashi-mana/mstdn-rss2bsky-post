#[derive(Debug)]
pub struct Media {
    pub url: String,
    pub file_size: usize,
    pub typ: String,
    pub rating: Rating,
}

#[derive(Debug)]
pub enum Rating {
    NonAdult,
    Other,
}

pub fn get_media(item: &rss::Item) -> Option<Media> {
    let media_content = {
        let media_opt = item
            .extensions
            .get("media")
            .and_then(|x| x.get("content"))
            .and_then(|x| x.get(0));
        match media_opt {
            Some(x) => x,
            None => return None,
        }
    };

    let file_size = match media_content.attrs.get("fileSize") {
        Some(x) => match x.parse() {
            Ok(parsed) => parsed,
            Err(err) => {
                eprintln!(
                    "Failed to parse the 'fileSize' attribute of the media content: {}",
                    err
                );
                return None;
            }
        },
        None => {
            eprintln!("Not found the 'fileSize' attribute of the media content.");
            return None;
        }
    };

    let typ = match media_content.attrs.get("type") {
        Some(x) => x,
        None => {
            eprintln!("Not found the 'type' attribute of the media content.");
            return None;
        }
    };

    let url = match media_content.attrs.get("url") {
        Some(x) => x,
        None => {
            eprintln!("Not found the 'url' attribute of the media content.");
            return None;
        }
    };

    let rating_ext = match media_content.children.get("rating").and_then(|x| x.get(0)) {
        Some(x) => x,
        None => {
            eprintln!("Not found the 'rating' content of the media content.");
            return None;
        }
    };

    let rating = match &rating_ext.value {
        Some(x) => match x.as_str() {
            "nonadult" => Rating::NonAdult,
            other => {
                eprintln!("Failed to parse the rating {}", other);
                Rating::Other
            }
        },
        None => {
            eprintln!("Not found the 'value' of the media rating content.");
            return None;
        }
    };

    Some(Media {
        url: url.to_string(),
        typ: typ.to_string(),
        file_size,
        rating,
    })
}

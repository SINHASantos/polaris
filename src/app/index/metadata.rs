use id3::TagLike;
use lewton::inside_ogg::OggStreamReader;
use log::error;
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils;
use crate::utils::AudioFormat;

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error(transparent)]
	Ape(#[from] ape::Error),
	#[error(transparent)]
	Id3(#[from] id3::Error),
	#[error("Filesystem error for `{0}`: `{1}`")]
	Io(PathBuf, std::io::Error),
	#[error(transparent)]
	Metaflac(#[from] metaflac::Error),
	#[error(transparent)]
	Mp4aMeta(#[from] mp4ameta::Error),
	#[error(transparent)]
	Opus(#[from] opus_headers::ParseError),
	#[error(transparent)]
	Vorbis(#[from] lewton::VorbisError),
	#[error("Could not find a Vorbis comment within flac file")]
	VorbisCommentNotFoundInFlacFile,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SongMetadata {
	pub disc_number: Option<u32>,
	pub track_number: Option<u32>,
	pub title: Option<String>,
	pub duration: Option<u32>,
	pub artists: Vec<String>,
	pub album_artists: Vec<String>,
	pub album: Option<String>,
	pub year: Option<i32>,
	pub has_artwork: bool,
	pub lyricists: Vec<String>,
	pub composers: Vec<String>,
	pub genres: Vec<String>,
	pub labels: Vec<String>,
}

pub fn read(path: &Path) -> Option<SongMetadata> {
	let data = match utils::get_audio_format(path) {
		Some(AudioFormat::AIFF) => read_id3(path),
		Some(AudioFormat::FLAC) => read_flac(path),
		Some(AudioFormat::MP3) => read_mp3(path),
		Some(AudioFormat::OGG) => read_vorbis(path),
		Some(AudioFormat::OPUS) => read_opus(path),
		Some(AudioFormat::WAVE) => read_id3(path),
		Some(AudioFormat::APE) | Some(AudioFormat::MPC) => read_ape(path),
		Some(AudioFormat::MP4) | Some(AudioFormat::M4B) => read_mp4(path),
		None => return None,
	};
	match data {
		Ok(d) => Some(d),
		Err(e) => {
			error!("Error while reading file metadata for '{:?}': {}", path, e);
			None
		}
	}
}

trait ID3Ext {
	fn get_text_values(&self, frame_name: &str) -> Vec<String>;
}

impl ID3Ext for id3::Tag {
	fn get_text_values(&self, frame_name: &str) -> Vec<String> {
		self.get(frame_name)
			.and_then(|f| f.content().text_values())
			.map(|i| i.map(str::to_string).collect())
			.unwrap_or_default()
	}
}

fn read_id3(path: &Path) -> Result<SongMetadata, Error> {
	let tag = id3::Tag::read_from_path(path).or_else(|error| {
		if let Some(tag) = error.partial_tag {
			Ok(tag)
		} else {
			Err(error)
		}
	})?;

	let artists = tag.get_text_values("TPE1");
	let album_artists = tag.get_text_values("TPE2");
	let album = tag.album().map(|s| s.to_string());
	let title = tag.title().map(|s| s.to_string());
	let duration = tag.duration();
	let disc_number = tag.disc();
	let track_number = tag.track();
	let year = tag
		.year()
		.or_else(|| tag.date_released().map(|d| d.year))
		.or_else(|| tag.original_date_released().map(|d| d.year))
		.or_else(|| tag.date_recorded().map(|d| d.year));
	let has_artwork = tag.pictures().count() > 0;
	let lyricists = tag.get_text_values("TEXT");
	let composers = tag.get_text_values("TCOM");
	let genres = tag.get_text_values("TCON");
	let labels = tag.get_text_values("TPUB");

	Ok(SongMetadata {
		disc_number,
		track_number,
		title,
		duration,
		artists,
		album_artists,
		album,
		year,
		has_artwork,
		lyricists,
		composers,
		genres,
		labels,
	})
}

fn read_mp3(path: &Path) -> Result<SongMetadata, Error> {
	let mut metadata = read_id3(path)?;
	let duration = {
		mp3_duration::from_path(path)
			.map(|d| d.as_secs() as u32)
			.ok()
	};
	metadata.duration = duration;
	Ok(metadata)
}

mod ape_ext {
	pub fn read_string(item: &ape::Item) -> Option<String> {
		match item.value {
			ape::ItemValue::Text(ref s) => Some(s.clone()),
			_ => None,
		}
	}

	pub fn read_strings(items: Vec<&ape::Item>) -> Vec<String> {
		items.iter().filter_map(|i| read_string(i)).collect()
	}

	pub fn read_i32(item: &ape::Item) -> Option<i32> {
		match item.value {
			ape::ItemValue::Text(ref s) => s.parse::<i32>().ok(),
			_ => None,
		}
	}

	pub fn read_x_of_y(item: &ape::Item) -> Option<u32> {
		match item.value {
			ape::ItemValue::Text(ref s) => {
				let format = regex::Regex::new(r#"^\d+"#).unwrap();
				if let Some(m) = format.find(s) {
					s[m.start()..m.end()].parse().ok()
				} else {
					None
				}
			}
			_ => None,
		}
	}
}

fn read_ape(path: &Path) -> Result<SongMetadata, Error> {
	let tag = ape::read_from_path(path)?;
	let artists = ape_ext::read_strings(tag.items("Artist"));
	let album = tag.item("Album").and_then(ape_ext::read_string);
	let album_artists = ape_ext::read_strings(tag.items("Album artist"));
	let title = tag.item("Title").and_then(ape_ext::read_string);
	let year = tag.item("Year").and_then(ape_ext::read_i32);
	let disc_number = tag.item("Disc").and_then(ape_ext::read_x_of_y);
	let track_number = tag.item("Track").and_then(ape_ext::read_x_of_y);
	let lyricists = ape_ext::read_strings(tag.items("LYRICIST"));
	let composers = ape_ext::read_strings(tag.items("COMPOSER"));
	let genres = ape_ext::read_strings(tag.items("GENRE"));
	let labels = ape_ext::read_strings(tag.items("PUBLISHER"));
	Ok(SongMetadata {
		artists,
		album_artists,
		album,
		title,
		duration: None,
		disc_number,
		track_number,
		year,
		has_artwork: false,
		lyricists,
		composers,
		genres,
		labels,
	})
}

fn read_vorbis(path: &Path) -> Result<SongMetadata, Error> {
	let file = fs::File::open(path).map_err(|e| Error::Io(path.to_owned(), e))?;
	let source = OggStreamReader::new(file)?;

	let mut metadata = SongMetadata::default();
	for (key, value) in source.comment_hdr.comment_list {
		utils::match_ignore_case! {
			match key {
				"TITLE" => metadata.title = Some(value),
				"ALBUM" => metadata.album = Some(value),
				"ARTIST" => metadata.artists.push(value),
				"ALBUMARTIST" => metadata.album_artists.push(value),
				"TRACKNUMBER" => metadata.track_number = value.parse::<u32>().ok(),
				"DISCNUMBER" => metadata.disc_number = value.parse::<u32>().ok(),
				"DATE" => metadata.year = value.parse::<i32>().ok(),
				"LYRICIST" => metadata.lyricists.push(value),
				"COMPOSER" => metadata.composers.push(value),
				"GENRE" => metadata.genres.push(value),
				"PUBLISHER" => metadata.labels.push(value),
				_ => (),
			}
		}
	}

	Ok(metadata)
}

fn read_opus(path: &Path) -> Result<SongMetadata, Error> {
	let headers = opus_headers::parse_from_path(path)?;

	let mut metadata = SongMetadata::default();
	for (key, value) in headers.comments.user_comments {
		utils::match_ignore_case! {
			match key {
				"TITLE" => metadata.title = Some(value),
				"ALBUM" => metadata.album = Some(value),
				"ARTIST" => metadata.artists.push(value),
				"ALBUMARTIST" => metadata.album_artists.push(value),
				"TRACKNUMBER" => metadata.track_number = value.parse::<u32>().ok(),
				"DISCNUMBER" => metadata.disc_number = value.parse::<u32>().ok(),
				"DATE" => metadata.year = value.parse::<i32>().ok(),
				"LYRICIST" => metadata.lyricists.push(value),
				"COMPOSER" => metadata.composers.push(value),
				"GENRE" => metadata.genres.push(value),
				"PUBLISHER" => metadata.labels.push(value),
				_ => (),
			}
		}
	}

	Ok(metadata)
}

fn read_flac(path: &Path) -> Result<SongMetadata, Error> {
	let tag = metaflac::Tag::read_from_path(path)?;
	let vorbis = tag
		.vorbis_comments()
		.ok_or(Error::VorbisCommentNotFoundInFlacFile)?;
	let disc_number = vorbis
		.get("DISCNUMBER")
		.and_then(|d| d[0].parse::<u32>().ok());
	let year = vorbis.get("DATE").and_then(|d| d[0].parse::<i32>().ok());
	let mut streaminfo = tag.get_blocks(metaflac::BlockType::StreamInfo);
	let duration = match streaminfo.next() {
		Some(metaflac::Block::StreamInfo(s)) => Some(s.total_samples as u32 / s.sample_rate),
		_ => None,
	};
	let has_artwork = tag.pictures().count() > 0;

	let multivalue = |o: Option<&Vec<String>>| o.cloned().unwrap_or_default();

	Ok(SongMetadata {
		artists: multivalue(vorbis.artist()),
		album_artists: multivalue(vorbis.album_artist()),
		album: vorbis.album().map(|v| v[0].clone()),
		title: vorbis.title().map(|v| v[0].clone()),
		duration,
		disc_number,
		track_number: vorbis.track(),
		year,
		has_artwork,
		lyricists: multivalue(vorbis.get("LYRICIST")),
		composers: multivalue(vorbis.get("COMPOSER")),
		genres: multivalue(vorbis.get("GENRE")),
		labels: multivalue(vorbis.get("PUBLISHER")),
	})
}

fn read_mp4(path: &Path) -> Result<SongMetadata, Error> {
	let mut tag = mp4ameta::Tag::read_from_path(path)?;
	let label_ident = mp4ameta::FreeformIdent::new("com.apple.iTunes", "Label");

	Ok(SongMetadata {
		artists: tag.take_artists().collect(),
		album_artists: tag.take_album_artists().collect(),
		album: tag.take_album(),
		title: tag.take_title(),
		duration: tag.duration().map(|v| v.as_secs() as u32),
		disc_number: tag.disc_number().map(|d| d as u32),
		track_number: tag.track_number().map(|d| d as u32),
		year: tag.year().and_then(|v| v.parse::<i32>().ok()),
		has_artwork: tag.artwork().is_some(),
		lyricists: tag.take_lyricists().collect(),
		composers: tag.take_composers().collect(),
		genres: tag.take_genres().collect(),
		labels: tag.take_strings_of(&label_ident).collect(),
	})
}

#[test]
fn reads_file_metadata() {
	let sample_tags = SongMetadata {
		disc_number: Some(3),
		track_number: Some(1),
		title: Some("TEST TITLE".into()),
		artists: vec!["TEST ARTIST".into()],
		album_artists: vec!["TEST ALBUM ARTIST".into()],
		album: Some("TEST ALBUM".into()),
		duration: None,
		year: Some(2016),
		has_artwork: false,
		lyricists: vec!["TEST LYRICIST".into()],
		composers: vec!["TEST COMPOSER".into()],
		genres: vec!["TEST GENRE".into()],
		labels: vec!["TEST LABEL".into()],
	};
	let flac_sample_tag = SongMetadata {
		duration: Some(0),
		..sample_tags.clone()
	};
	let mp3_sample_tag = SongMetadata {
		duration: Some(0),
		..sample_tags.clone()
	};
	let m4a_sample_tag = SongMetadata {
		duration: Some(0),
		..sample_tags.clone()
	};
	assert_eq!(
		read(Path::new("test-data/formats/sample.aif")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.mp3")).unwrap(),
		mp3_sample_tag
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.ogg")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.flac")).unwrap(),
		flac_sample_tag
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.m4a")).unwrap(),
		m4a_sample_tag
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.opus")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.ape")).unwrap(),
		sample_tags
	);
	assert_eq!(
		read(Path::new("test-data/formats/sample.wav")).unwrap(),
		sample_tags
	);
}

#[test]
fn reads_embedded_artwork() {
	assert!(
		read(Path::new("test-data/artwork/sample.aif"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.mp3"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.flac"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.m4a"))
			.unwrap()
			.has_artwork
	);
	assert!(
		read(Path::new("test-data/artwork/sample.wav"))
			.unwrap()
			.has_artwork
	);
}

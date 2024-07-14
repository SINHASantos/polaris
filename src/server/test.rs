use bytes::Bytes;
use http::response::Builder;
use http::{Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::ops::Deref;
use std::path::Path;
use std::time::Duration;

pub mod constants;
pub mod protocol;

mod admin;
mod auth;
mod collection;
mod ddns;
mod lastfm;
mod media;
mod playlist;
mod settings;
mod swagger;
mod user;
mod web;

use crate::app::index;
use crate::server::dto;
use crate::server::test::constants::*;

pub use crate::server::axum::test::ServiceType;

pub trait TestService {
	async fn new(test_name: &str) -> Self;

	async fn execute_request<T: Serialize + Clone + 'static>(
		&mut self,
		request: &Request<T>,
	) -> (Builder, Option<Bytes>);

	async fn fetch<T: Serialize + Clone + 'static>(
		&mut self,
		request: &Request<T>,
	) -> Response<()> {
		let (response_builder, _body) = self.execute_request(request).await;
		response_builder.body(()).unwrap()
	}

	async fn fetch_bytes<T: Serialize + Clone + 'static>(
		&mut self,
		request: &Request<T>,
	) -> Response<Vec<u8>> {
		let (response_builder, body) = self.execute_request(request).await;
		response_builder
			.body(body.unwrap().deref().to_owned())
			.unwrap()
	}

	async fn fetch_json<T: Serialize + Clone + 'static, U: DeserializeOwned>(
		&mut self,
		request: &Request<T>,
	) -> Response<U> {
		let (response_builder, body) = self.execute_request(request).await;
		let body = serde_json::from_slice(&body.unwrap()).unwrap();
		response_builder.body(body).unwrap()
	}

	async fn complete_initial_setup(&mut self) {
		let configuration = dto::Config {
			users: Some(vec![
				dto::NewUser {
					name: TEST_USERNAME_ADMIN.into(),
					password: TEST_PASSWORD_ADMIN.into(),
					admin: true,
				},
				dto::NewUser {
					name: TEST_USERNAME.into(),
					password: TEST_PASSWORD.into(),
					admin: false,
				},
			]),
			mount_dirs: Some(vec![dto::MountDir {
				name: TEST_MOUNT_NAME.into(),
				source: TEST_MOUNT_SOURCE.into(),
			}]),
			..Default::default()
		};
		let request = protocol::apply_config(configuration);
		let response = self.fetch(&request).await;
		assert_eq!(response.status(), StatusCode::OK);
	}

	async fn login_internal(&mut self, username: &str, password: &str) {
		let request = protocol::login(username, password);
		let response = self.fetch_json::<_, dto::Authorization>(&request).await;
		assert_eq!(response.status(), StatusCode::OK);
		let authorization = response.into_body();
		self.set_authorization(Some(authorization));
	}

	async fn login_admin(&mut self) {
		self.login_internal(TEST_USERNAME_ADMIN, TEST_PASSWORD_ADMIN)
			.await;
	}

	async fn login(&mut self) {
		self.login_internal(TEST_USERNAME, TEST_PASSWORD).await;
	}

	async fn logout(&mut self) {
		self.set_authorization(None);
	}

	fn set_authorization(&mut self, authorization: Option<dto::Authorization>);

	async fn index(&mut self) {
		let request = protocol::trigger_index();
		let response = self.fetch(&request).await;
		assert_eq!(response.status(), StatusCode::OK);

		loop {
			let browse_request = protocol::browse(Path::new(""));
			let response = self
				.fetch_json::<(), Vec<index::CollectionFile>>(&browse_request)
				.await;
			let entries = response.body();
			if !entries.is_empty() {
				break;
			}
			tokio::time::sleep(Duration::from_millis(100)).await;
		}

		loop {
			let flatten_request = protocol::flatten(Path::new(""));
			let response = self
				.fetch_json::<_, Vec<index::Song>>(&flatten_request)
				.await;
			let entries = response.body();
			if !entries.is_empty() {
				break;
			}
			tokio::time::sleep(Duration::from_millis(100)).await;
		}
	}
}

fn add_trailing_slash<T>(request: &mut Request<T>) {
	*request.uri_mut() = (request.uri().to_string().trim_end_matches('/').to_string() + "/")
		.parse()
		.unwrap();
}

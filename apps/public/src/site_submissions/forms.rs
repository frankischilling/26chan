//! Text-only source multipart forms with a bounded body and exact field names.
use axum::{
    body::{Body, to_bytes},
    extract::{FromRequest, Multipart, Request},
    http::{StatusCode, header},
};
use std::collections::BTreeMap;

const MAX_BODY: usize = 131_072;
pub(super) struct Fields(BTreeMap<String, String>);
impl Fields {
    pub(super) async fn read(request: Request, names: &[&str]) -> Result<Self, StatusCode> {
        if request
            .headers()
            .get_all(header::CONTENT_TYPE)
            .iter()
            .count()
            != 1
        {
            return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
        let (parts, body) = request.into_parts();
        let bytes = to_bytes(body, MAX_BODY)
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        let mut multipart =
            Multipart::from_request(Request::from_parts(parts, Body::from(bytes)), &())
                .await
                .map_err(|_| StatusCode::BAD_REQUEST)?;
        let mut fields = BTreeMap::new();
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?
        {
            let name = field.name().ok_or(StatusCode::BAD_REQUEST)?.to_owned();
            if field.file_name().is_some()
                || !names.contains(&name.as_str())
                || fields.contains_key(&name)
            {
                return Err(StatusCode::BAD_REQUEST);
            }
            let bytes = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            let value = String::from_utf8(bytes.to_vec()).map_err(|_| StatusCode::BAD_REQUEST)?;
            fields.insert(name, value);
        }
        if fields.len() != names.len() {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(Self(fields))
    }
    pub(super) fn get(&self, name: &str) -> &str {
        self.0.get(name).map_or("", String::as_str)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(parts: &[(&str, &str, &str)]) -> Request {
        let mut body = String::new();
        for (name, attributes, value) in parts {
            body.push_str(&format!("--owned\r\nContent-Disposition: form-data; name=\"{name}\"{attributes}\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned--\r\n");
        Request::post("/feedback")
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=owned")
            .body(Body::from(body))
            .unwrap()
    }
    #[tokio::test]
    async fn literal_text_is_preserved_and_ambiguous_or_file_inputs_refuse() {
        let fields = Fields::read(
            request(&[("message", "", "<b>literal</b>\r\nsecond")]),
            &["message"],
        )
        .await
        .unwrap();
        assert_eq!(fields.get("message"), "<b>literal</b>\r\nsecond");
        for parts in [
            vec![],
            vec![("unknown", "", "x")],
            vec![("message", "; filename=\"\"", "x")],
            vec![("message", "", "one"), ("message", "", "two")],
        ] {
            assert!(Fields::read(request(&parts), &["message"]).await.is_err());
        }
        let large = "x".repeat(MAX_BODY);
        assert!(matches!(
            Fields::read(request(&[("message", "", &large)]), &["message"]).await,
            Err(StatusCode::PAYLOAD_TOO_LARGE)
        ));
        let mut duplicate = request(&[("message", "", "x")]);
        duplicate.headers_mut().append(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=owned".parse().unwrap(),
        );
        assert!(matches!(
            Fields::read(duplicate, &["message"]).await,
            Err(StatusCode::UNSUPPORTED_MEDIA_TYPE)
        ));
    }
}

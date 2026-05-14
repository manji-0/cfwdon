use crate::{Request, Result};

pub(crate) fn parse_relationship_query_ids(req: &Request) -> Result<Vec<String>> {
    let url = req.url()?;
    let mut ids = Vec::new();

    for (key, value) in url.query_pairs() {
        if key == "id[]" || key == "id" {
            let value = value.trim().to_owned();
            if !value.is_empty() {
                ids.push(value);
            }
        }
    }

    Ok(ids)
}

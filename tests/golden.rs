use anyhow::{Result, anyhow};
use musicindex_live_publisher::{DropFile, PaymentRoute, SCHEMA_VERSION, payload_from_dropfile};
use serde_json::{Map, Value, json};

const HGH_EXAMPLE_2: &str = include_str!("fixtures/hgh-example-2.json");
const HGH_EXAMPLE_3: &str = include_str!("fixtures/hgh-example-3.json");

fn dropfile_from_reference(reference: &Value) -> Result<DropFile> {
    let destinations = reference["value"]["destinations"]
        .as_array()
        .ok_or_else(|| anyhow!("reference destinations should be an array"))?;

    let value_routes = destinations
        .iter()
        .map(payment_route_from_destination)
        .collect::<Result<Vec<_>>>()?;

    Ok(DropFile {
        schema: SCHEMA_VERSION.to_owned(),
        target: "default".to_owned(),
        artist: reference["line"]
            .as_array()
            .and_then(|line| line.last())
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        title: required_string(reference, "title")?.to_owned(),
        duration_secs: reference.get("duration").and_then(Value::as_f64),
        image: reference
            .get("image")
            .and_then(Value::as_str)
            .map(str::to_owned),
        feed_guid: reference
            .get("feedGuid")
            .and_then(Value::as_str)
            .map(str::to_owned),
        track_guid: reference
            .get("itemGuid")
            .and_then(Value::as_str)
            .map(str::to_owned),
        value_routes,
        value_routes_source: Some("golden-reference".to_owned()),
    })
}

fn payment_route_from_destination(destination: &Value) -> Result<PaymentRoute> {
    Ok(PaymentRoute {
        recipient_name: destination
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        route_type: destination
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        split: destination
            .get("split")
            .and_then(Value::as_str)
            .map(str::parse::<f64>)
            .transpose()?,
        fee: destination.get("fee").and_then(Value::as_bool),
        address: destination
            .get("address")
            .and_then(Value::as_str)
            .map(str::to_owned),
        custom_key: destination
            .get("customKey")
            .and_then(Value::as_str)
            .map(str::to_owned),
        custom_value: destination
            .get("customValue")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("reference {key} should be a string"))
}

fn expected_sourceable_payload(reference: &Value) -> Result<Value> {
    Ok(json!({
        "title": required_string(reference, "title")?,
        "image": required_string(reference, "image")?,
        "description": "",
        "type": "music",
        "startTime": 0,
        "duration": reference["duration"],
        "eventGuid": required_string(reference, "eventGuid")?,
        "blockGuid": required_string(reference, "blockGuid")?,
        "feedGuid": required_string(reference, "feedGuid")?,
        "itemGuid": required_string(reference, "itemGuid")?,
        "value": {
            "model": {
                "type": required_string(&reference["value"]["model"], "type")?,
                "method": required_string(&reference["value"]["model"], "method")?
            },
            "destinations": reference["value"]["destinations"].clone()
        }
    }))
}

fn assert_payload_matches_sourceable_reference(reference_json: &str) -> Result<()> {
    let reference: Value = serde_json::from_str(reference_json)?;
    let dropfile = dropfile_from_reference(&reference)?;
    let event_guid = required_string(&reference, "eventGuid")?;
    let block_guid = required_string(&reference, "blockGuid")?;

    let assembled = serde_json::to_value(payload_from_dropfile(&dropfile, event_guid, block_guid))?;

    assert_eq!(assembled, expected_sourceable_payload(&reference)?);
    assert_destination_shape_matches(&assembled, &reference)?;
    assert_payload_is_direct(&assembled)?;
    Ok(())
}

fn assert_destination_shape_matches(assembled: &Value, reference: &Value) -> Result<()> {
    let assembled_destinations = assembled["value"]["destinations"]
        .as_array()
        .ok_or_else(|| anyhow!("assembled destinations should be an array"))?;
    let reference_destinations = reference["value"]["destinations"]
        .as_array()
        .ok_or_else(|| anyhow!("reference destinations should be an array"))?;

    assert_eq!(assembled_destinations.len(), reference_destinations.len());

    for (assembled_destination, reference_destination) in assembled_destinations
        .iter()
        .zip(reference_destinations.iter())
    {
        assert_eq!(
            object_keys(assembled_destination)?,
            object_keys(reference_destination)?
        );
        assert!(assembled_destination["split"].is_string());
        assert_eq!(
            assembled_destination["split"],
            reference_destination["split"]
        );
    }

    Ok(())
}

fn object_keys(value: &Value) -> Result<Vec<&str>> {
    let object: &Map<String, Value> = value
        .as_object()
        .ok_or_else(|| anyhow!("value should be an object"))?;
    Ok(object.keys().map(String::as_str).collect())
}

fn assert_payload_is_direct(payload: &Value) -> Result<()> {
    let keys = object_keys(payload)?;

    assert_ne!(keys, vec!["event_id", "metadata"]);
    assert!(payload.get("event_id").is_none());
    assert!(payload.get("metadata").is_none());
    Ok(())
}

#[test]
fn golden_hgh_example_2_matches_sourceable_live_value_payload() -> Result<()> {
    assert_payload_matches_sourceable_reference(HGH_EXAMPLE_2)
}

#[test]
fn golden_hgh_example_3_matches_sourceable_live_value_payload() -> Result<()> {
    assert_payload_matches_sourceable_reference(HGH_EXAMPLE_3)
}

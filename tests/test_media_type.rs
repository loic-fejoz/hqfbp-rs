use hqfbp_rs::{Header, MediaType, pack, unpack};

#[test]
fn test_media_type_canonicalization() {
    let mt = MediaType::Type("application/json".to_string());
    let canonical = mt.canonicalize();
    assert_eq!(canonical, MediaType::Format(50));
    assert_eq!(canonical.to_mime(), "application/json");
}

#[test]
fn test_header_media_type_optimization() {
    let header = Header {
        message_id: Some(123),
        // Before packing, it's what we set (modulo canonicalization if we call it)
        // But set_media_type calls canonicalize internally!
        // We can't easily simulate set_media_type with struct init unless we just set fields.
        // set_media_type does: self.content_type = ...; self.content_format = ...; self.canonicalize();
        // If we want to use struct init, we must manually set the resulting fields.
        // Or keep mutable and fix how we assign?
        // Clippy complained about:
        // header.message_id = Some(123);
        // header.set_media_type(...)
        // "field assignment outside...".
        // Use struct update for message_id.
        ..Default::default()
    };
    // Re-declare as mut if we need to set media type?
    // Wait, set_media_type takes &mut self.
    // So header must be mut.
    let mut header = header;
    header.set_media_type(Some(MediaType::Type("application/json".to_string())));

    // Before packing, it's what we set (modulo canonicalization if we call it)
    // But set_media_type calls canonicalize internally!
    assert_eq!(header.content_format, Some(50));
    assert_eq!(header.content_type, None);

    let packed = pack(&header, b"{}").expect("Pack failed");
    let (unpacked, _) = unpack(packed).expect("Unpack failed");

    assert_eq!(unpacked.content_format, Some(50));
    assert_eq!(unpacked.content_type, None);
    assert_eq!(unpacked.media_type(), Some(MediaType::Format(50)));
}

#[test]
fn test_header_unknown_mime() {
    let mut header = Header {
        message_id: Some(123),
        ..Default::default()
    };
    header.set_media_type(Some(MediaType::Type("application/x-custom".to_string())));

    assert_eq!(header.content_format, None);
    assert_eq!(
        header.content_type,
        Some("application/x-custom".to_string())
    );

    let packed = pack(&header, b"data").expect("Pack failed");
    let (unpacked, _) = unpack(packed).expect("Unpack failed");

    assert_eq!(
        unpacked.content_type,
        Some("application/x-custom".to_string())
    );
    assert_eq!(
        unpacked.media_type(),
        Some(MediaType::Type("application/x-custom".to_string()))
    );
}

#[test]
fn test_media_type_display() {
    let mt = MediaType::Format(42);
    assert_eq!(format!("{mt}"), "application/octet-stream");
}

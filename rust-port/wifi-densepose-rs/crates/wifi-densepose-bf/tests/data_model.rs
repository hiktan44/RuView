//! Integration tests for the 802.11bf-aligned data model.

use wifi_densepose_bf::{
    from_vendor_csi, BfError, CsiPayload, MeasurementSchedule, MeasurementSetupId,
    NegotiatedSession, SensingBand, SensingCapability, SensingMeasurementReport, SensingRole,
    SoundingType, SubcarrierMeasurement, VendorCsiFrame,
};

fn initiator_cap() -> SensingCapability {
    SensingCapability::new(
        vec![SensingBand::Sub7GHz, SensingBand::Dmg60GHz],
        4,
        114,
        100.0,
        true,
        vec![SensingRole::Initiator, SensingRole::Responder],
    )
}

fn responder_cap() -> SensingCapability {
    SensingCapability::new(
        vec![SensingBand::Sub7GHz],
        2,
        56,
        50.0,
        true,
        vec![SensingRole::Responder],
    )
}

#[test]
fn negotiation_intersects_capabilities() {
    let session = SensingCapability::negotiate(&initiator_cap(), &responder_cap())
        .expect("compatible peers should negotiate");

    // Common band only: Sub7GHz (responder lacks 60 GHz).
    assert_eq!(session.bands, vec![SensingBand::Sub7GHz]);
    // Minimum of antenna / subcarrier / rate.
    assert_eq!(session.antenna_count, 2);
    assert_eq!(session.subcarrier_resolution, 56);
    assert!((session.measurement_rate_hz - 50.0).abs() < f32::EPSILON);
    // Both support trigger-based.
    assert!(session.trigger_based);
    // Common role is Responder; initiator/responder pair still valid.
    assert_eq!(session.roles, vec![SensingRole::Responder]);
    assert!(session.supports_csi());
}

#[test]
fn negotiation_fails_with_no_common_band() {
    let local = SensingCapability::new(
        vec![SensingBand::Dmg60GHz],
        2,
        16,
        20.0,
        false,
        vec![SensingRole::Initiator],
    );
    let remote = SensingCapability::new(
        vec![SensingBand::Sub7GHz],
        2,
        56,
        50.0,
        false,
        vec![SensingRole::Responder],
    );
    let err = SensingCapability::negotiate(&local, &remote).unwrap_err();
    assert!(matches!(err, BfError::NegotiationFailed(_)));
}

#[test]
fn negotiation_fails_without_initiator_responder_pair() {
    // Both only responders: no one can initiate.
    let a = SensingCapability::new(
        vec![SensingBand::Sub7GHz],
        2,
        56,
        50.0,
        true,
        vec![SensingRole::Responder],
    );
    let b = a.clone();
    let err = SensingCapability::negotiate(&a, &b).unwrap_err();
    assert!(matches!(err, BfError::NegotiationFailed(_)));
}

#[test]
fn trigger_based_schedule_models_tdm_aggregator() {
    let setup = MeasurementSetupId::new(1);
    let responders = vec![
        MeasurementSetupId::new(10),
        MeasurementSetupId::new(11),
        MeasurementSetupId::new(12),
    ];
    let sched = MeasurementSchedule::new(setup, 50.0, responders, 50.0)
        .expect("valid schedule within negotiated rate");
    assert_eq!(sched.responder_count(), 3);
    assert_eq!(sched.interval_us(), 20_000); // 1e6 / 50 Hz
}

#[test]
fn schedule_rejects_rate_above_negotiated_max() {
    let err = MeasurementSchedule::new(
        MeasurementSetupId::new(1),
        100.0,
        vec![MeasurementSetupId::new(2)],
        50.0,
    )
    .unwrap_err();
    assert!(matches!(err, BfError::InvalidSchedule(_)));
}

#[test]
fn schedule_rejects_nonpositive_rate_and_empty_responders() {
    assert!(matches!(
        MeasurementSchedule::new(
            MeasurementSetupId::new(1),
            0.0,
            vec![MeasurementSetupId::new(2)],
            50.0
        )
        .unwrap_err(),
        BfError::InvalidSchedule(_)
    ));
    assert!(matches!(
        MeasurementSchedule::new(MeasurementSetupId::new(1), 10.0, vec![], 50.0).unwrap_err(),
        BfError::InvalidSchedule(_)
    ));
}

#[test]
fn vendor_csi_round_trips_into_report() {
    let frame = VendorCsiFrame {
        node_id: 7,
        amplitudes: vec![1.0, 0.5, 0.25],
        phases: vec![0.0, 1.0, -1.0],
        subcarrier_count: 3,
        timestamp_us: 123_456,
        antenna_count: 1,
    };
    let report = from_vendor_csi(&frame).expect("well-formed vendor frame ingests");

    assert_eq!(report.measurement_setup_id, MeasurementSetupId::new(7));
    assert_eq!(report.role, SensingRole::Responder);
    assert_eq!(report.band, SensingBand::Sub7GHz);
    assert_eq!(report.sounding_type, SoundingType::TrafficDerived);
    assert_eq!(report.timestamp_us, 123_456);
    assert_eq!(report.subcarrier_count(), 3);

    // Amplitude/phase preserved.
    assert!((report.payload.subcarriers[0].amplitude - 1.0).abs() < f32::EPSILON);
    assert!((report.payload.subcarriers[1].phase - 1.0).abs() < f32::EPSILON);
}

#[test]
fn vendor_csi_rejects_length_mismatch() {
    let frame = VendorCsiFrame {
        node_id: 1,
        amplitudes: vec![1.0, 0.5],
        phases: vec![0.0],
        subcarrier_count: 2,
        timestamp_us: 0,
        antenna_count: 1,
    };
    assert!(matches!(from_vendor_csi(&frame).unwrap_err(), BfError::Ingest(_)));
}

#[test]
fn vendor_csi_rejects_subcarrier_count_disagreement() {
    let frame = VendorCsiFrame {
        node_id: 1,
        amplitudes: vec![1.0, 0.5],
        phases: vec![0.0, 0.1],
        subcarrier_count: 5,
        timestamp_us: 0,
        antenna_count: 1,
    };
    assert!(matches!(from_vendor_csi(&frame).unwrap_err(), BfError::Ingest(_)));
}

#[test]
fn report_validation_rejects_malformed() {
    // Empty payload.
    assert!(matches!(
        SensingMeasurementReport::new(
            MeasurementSetupId::new(1),
            SensingRole::Responder,
            SensingBand::Sub7GHz,
            0,
            SoundingType::NdpSounding,
            1,
            CsiPayload::new(vec![]),
        )
        .unwrap_err(),
        BfError::InvalidReport(_)
    ));

    // Zero antennas.
    assert!(matches!(
        SensingMeasurementReport::new(
            MeasurementSetupId::new(1),
            SensingRole::Responder,
            SensingBand::Sub7GHz,
            0,
            SoundingType::NdpSounding,
            0,
            CsiPayload::new(vec![SubcarrierMeasurement::new(1.0, 0.0)]),
        )
        .unwrap_err(),
        BfError::InvalidReport(_)
    ));

    // Non-finite sample.
    assert!(matches!(
        SensingMeasurementReport::new(
            MeasurementSetupId::new(1),
            SensingRole::Responder,
            SensingBand::Sub7GHz,
            0,
            SoundingType::NdpSounding,
            1,
            CsiPayload::new(vec![SubcarrierMeasurement::new(f32::NAN, 0.0)]),
        )
        .unwrap_err(),
        BfError::InvalidReport(_)
    ));
}

#[test]
fn report_serde_round_trip() {
    let report = SensingMeasurementReport::new(
        MeasurementSetupId::new(42),
        SensingRole::Initiator,
        SensingBand::Sub7GHz,
        999,
        SoundingType::NdpSounding,
        2,
        CsiPayload::new(vec![
            SubcarrierMeasurement::new(1.0, 0.0),
            SubcarrierMeasurement::new(0.7, 0.5),
        ]),
    )
    .expect("valid report");

    let json = serde_json::to_string(&report).expect("serialize");
    let back: SensingMeasurementReport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(report, back);
    back.validate().expect("round-tripped report stays valid");
}

#[test]
fn negotiated_session_serde_round_trip() {
    let session: NegotiatedSession =
        SensingCapability::negotiate(&initiator_cap(), &responder_cap()).unwrap();
    let json = serde_json::to_string(&session).expect("serialize");
    let back: NegotiatedSession = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(session, back);
}

#[test]
fn role_counterpart_is_symmetric() {
    assert_eq!(SensingRole::Initiator.counterpart(), SensingRole::Responder);
    assert_eq!(SensingRole::Responder.counterpart(), SensingRole::Initiator);
}

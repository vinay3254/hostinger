use deploy_platform::agent_protocol::{
    AgentCommand, AgentEnvelope, AgentProtocolError, AgentProtocolValidator, CreateReleaseCommand,
    NodeCredentials, NodeTokenManager, AGENT_PROTOCOL_VERSION, MAX_PAYLOAD_SIZE_BYTES,
};
use std::collections::HashMap;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[test]
fn test_protocol_version_mismatch_rejected() {
    let creds = NodeCredentials::generate(Uuid::new_v4());
    let mut validator = AgentProtocolValidator::new(Duration::from_secs(300));
    validator.register_credentials(creds.clone());

    let command = AgentCommand::Health;
    let envelope = AgentEnvelope::new_signed(
        "v2".to_string(), // Invalid version
        Uuid::new_v4(),
        creds.node_id,
        OffsetDateTime::now_utc(),
        command,
        &creds.secret_key,
    )
    .expect("envelope creation should succeed");

    let result = validator.validate_and_unpack(&envelope);
    match result {
        Err(AgentProtocolError::VersionMismatch { expected, actual }) => {
            assert_eq!(expected, AGENT_PROTOCOL_VERSION);
            assert_eq!(actual, "v2");
        }
        other => panic!("expected VersionMismatch, got {:?}", other),
    }
}

#[test]
fn test_operation_id_deduplication() {
    let creds = NodeCredentials::generate(Uuid::new_v4());
    let mut validator = AgentProtocolValidator::new(Duration::from_secs(300));
    validator.register_credentials(creds.clone());

    let op_id = Uuid::new_v4();
    let command = AgentCommand::Health;

    let envelope1 = AgentEnvelope::new_signed(
        AGENT_PROTOCOL_VERSION.to_string(),
        op_id,
        creds.node_id,
        OffsetDateTime::now_utc(),
        command.clone(),
        &creds.secret_key,
    )
    .expect("envelope creation should succeed");

    // First attempt succeeds
    let first = validator.validate_and_unpack(&envelope1);
    assert!(first.is_ok(), "first operation attempt must succeed");

    // Duplicate attempt with same operation_id must be rejected
    let envelope2 = AgentEnvelope::new_signed(
        AGENT_PROTOCOL_VERSION.to_string(),
        op_id,
        creds.node_id,
        OffsetDateTime::now_utc(),
        command,
        &creds.secret_key,
    )
    .expect("envelope creation should succeed");

    let second = validator.validate_and_unpack(&envelope2);
    match second {
        Err(AgentProtocolError::ReplayDetected(rejected_id)) => {
            assert_eq!(rejected_id, op_id);
        }
        other => panic!("expected ReplayDetected, got {:?}", other),
    }
}

#[test]
fn test_hmac_signature_verification_and_tamper_rejection() {
    let creds = NodeCredentials::generate(Uuid::new_v4());
    let wrong_creds = NodeCredentials::generate(Uuid::new_v4());
    let mut validator = AgentProtocolValidator::new(Duration::from_secs(300));
    validator.register_credentials(creds.clone());

    let command = AgentCommand::DrainRelease {
        release_id: Uuid::new_v4(),
        drain_timeout_secs: 15,
    };

    // Valid envelope
    let valid_envelope = AgentEnvelope::new_signed(
        AGENT_PROTOCOL_VERSION.to_string(),
        Uuid::new_v4(),
        creds.node_id,
        OffsetDateTime::now_utc(),
        command.clone(),
        &creds.secret_key,
    )
    .expect("signing should succeed");

    assert!(validator.validate_and_unpack(&valid_envelope).is_ok());

    // Tampered payload
    let mut tampered_envelope = valid_envelope.clone();
    tampered_envelope.operation_id = Uuid::new_v4(); // changed after signing
    let err = validator.validate_and_unpack(&tampered_envelope);
    assert!(matches!(err, Err(AgentProtocolError::InvalidSignature)));

    // Signed with wrong key
    let wrong_sig_envelope = AgentEnvelope::new_signed(
        AGENT_PROTOCOL_VERSION.to_string(),
        Uuid::new_v4(),
        creds.node_id,
        OffsetDateTime::now_utc(),
        command,
        &wrong_creds.secret_key,
    )
    .expect("signing should succeed");

    let err2 = validator.validate_and_unpack(&wrong_sig_envelope);
    assert!(matches!(err2, Err(AgentProtocolError::InvalidSignature)));
}

#[test]
fn test_timestamp_drift_rejection() {
    let creds = NodeCredentials::generate(Uuid::new_v4());
    let mut validator = AgentProtocolValidator::new(Duration::from_secs(300));
    validator.register_credentials(creds.clone());

    // Expired timestamp (10 minutes in the past)
    let expired_envelope = AgentEnvelope::new_signed(
        AGENT_PROTOCOL_VERSION.to_string(),
        Uuid::new_v4(),
        creds.node_id,
        OffsetDateTime::now_utc() - time::Duration::seconds(600),
        AgentCommand::Health,
        &creds.secret_key,
    )
    .expect("signing should succeed");

    let err = validator.validate_and_unpack(&expired_envelope);
    assert!(matches!(
        err,
        Err(AgentProtocolError::TimestampExpired { .. })
    ));
}

#[test]
fn test_forbidden_command_and_safety_validation() {
    let creds = NodeCredentials::generate(Uuid::new_v4());
    let mut validator = AgentProtocolValidator::new(Duration::from_secs(300));
    validator.register_credentials(creds.clone());

    // Path traversal in image path
    let unsafe_command = AgentCommand::CreateRelease(CreateReleaseCommand {
        release_id: Uuid::new_v4(),
        deployment_id: Uuid::new_v4(),
        image_path: "../../../etc/passwd".to_string(),
        env: HashMap::new(),
        cpu_limit_millicores: 1000,
        memory_limit_bytes: 512 * 1024 * 1024,
    });

    let envelope = AgentEnvelope::new_signed(
        AGENT_PROTOCOL_VERSION.to_string(),
        Uuid::new_v4(),
        creds.node_id,
        OffsetDateTime::now_utc(),
        unsafe_command,
        &creds.secret_key,
    )
    .expect("signing should succeed");

    let err = validator.validate_and_unpack(&envelope);
    assert!(matches!(err, Err(AgentProtocolError::ForbiddenCommand(..))));
}

#[test]
fn test_bounded_request_size() {
    let creds = NodeCredentials::generate(Uuid::new_v4());
    let mut validator = AgentProtocolValidator::new(Duration::from_secs(300));
    validator.register_credentials(creds.clone());

    // Create huge env payload exceeding 1MB
    let mut huge_env = HashMap::new();
    for i in 0..10_000 {
        huge_env.insert(format!("KEY_{}", i), "A".repeat(128));
    }

    let command = AgentCommand::CreateRelease(CreateReleaseCommand {
        release_id: Uuid::new_v4(),
        deployment_id: Uuid::new_v4(),
        image_path: "/var/lib/deploy-platform/artifacts/valid.tar.gz".to_string(),
        env: huge_env,
        cpu_limit_millicores: 1000,
        memory_limit_bytes: 512 * 1024 * 1024,
    });

    let envelope_result = AgentEnvelope::new_signed(
        AGENT_PROTOCOL_VERSION.to_string(),
        Uuid::new_v4(),
        creds.node_id,
        OffsetDateTime::now_utc(),
        command,
        &creds.secret_key,
    );

    match envelope_result {
        Err(AgentProtocolError::PayloadTooLarge { size, max }) => {
            assert!(size > max);
            assert_eq!(max, MAX_PAYLOAD_SIZE_BYTES);
        }
        Ok(envelope) => {
            let val_result = validator.validate_and_unpack(&envelope);
            assert!(matches!(
                val_result,
                Err(AgentProtocolError::PayloadTooLarge { .. })
            ));
        }
        Err(other) => panic!("expected PayloadTooLarge, got {:?}", other),
    }
}

#[test]
fn test_node_token_lifecycle_rotation_and_revocation() {
    let node_id = Uuid::new_v4();
    let mut token_manager = NodeTokenManager::new();

    // Initial credentials
    let initial_creds = token_manager.issue_token(node_id);
    assert_eq!(initial_creds.node_id, node_id);
    assert_eq!(initial_creds.epoch, 1);
    assert!(token_manager.is_valid(&initial_creds));

    // Rotate token
    let rotated_creds = token_manager
        .rotate_token(node_id)
        .expect("rotation succeeds");
    assert_eq!(rotated_creds.epoch, 2);
    assert_ne!(rotated_creds.secret_key, initial_creds.secret_key);
    assert!(token_manager.is_valid(&rotated_creds));

    // Old token should be invalidated after rotation
    assert!(!token_manager.is_valid(&initial_creds));

    // Revoke node
    token_manager.revoke_node(node_id);
    assert!(!token_manager.is_valid(&rotated_creds));
}

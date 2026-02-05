use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cggmp24::supported_curves::Secp256k1;
use sha2::Sha256;
use tokio::sync::Barrier;

use crate::test::mock::{
    AuxParty, KeygenNetworkCoordinator, KeygenParty, NetworkCoordinator, SigningNetworkCoordinator,
    SigningParty,
};

/// Helper function to get current timestamp
fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 20)]
async fn test_signing_3_parties_threshold_2() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt::try_init();

    tracing::info!("🧪 Testing Signing with 3 parties (2-of-3 threshold)");

    let n = 3u16;
    let t = 2u16;
    let aux_timestamp = get_timestamp();
    let keygen_timestamp = aux_timestamp + 1;
    let signing_timestamp = keygen_timestamp + 1;

    // Phase 1: Generate aux info
    tracing::info!("Phase 1: Generating aux info");
    let aux_coordinator = NetworkCoordinator::new();

    let mut alice = AuxParty::new(0, aux_coordinator.clone()).await;
    let mut bob = AuxParty::new(1, aux_coordinator.clone()).await;
    let mut charlie = AuxParty::new(2, aux_coordinator.clone()).await;

    // Create a barrier to ensure all parties start simultaneously
    let barrier = Arc::new(Barrier::new(3));

    let barrier_clone = barrier.clone();
    let alice_handle = tokio::spawn(async move {
        barrier_clone.wait().await;
        alice.generate_aux_info(n, aux_timestamp).await.unwrap()
    });

    let barrier_clone = barrier.clone();
    let bob_handle = tokio::spawn(async move {
        barrier_clone.wait().await;
        bob.generate_aux_info(n, aux_timestamp).await.unwrap()
    });

    let barrier_clone = barrier.clone();
    let charlie_handle = tokio::spawn(async move {
        barrier_clone.wait().await;
        charlie.generate_aux_info(n, aux_timestamp).await.unwrap()
    });

    // Wait for all parties concurrently
    let (alice_aux, bob_aux, charlie_aux) = tokio::join!(alice_handle, bob_handle, charlie_handle);
    let alice_aux = alice_aux.unwrap();
    let bob_aux = bob_aux.unwrap();
    let charlie_aux = charlie_aux.unwrap();

    // Phase 2: Generate key shares
    tracing::info!("Phase 2: Generating key shares");
    let keygen_coordinator = KeygenNetworkCoordinator::new();

    let mut alice = KeygenParty::new(0, keygen_coordinator.clone(), alice_aux).await;
    let mut bob = KeygenParty::new(1, keygen_coordinator.clone(), bob_aux).await;
    let mut charlie = KeygenParty::new(2, keygen_coordinator.clone(), charlie_aux).await;

    let alice_handle = tokio::spawn(async move {
        alice
            .generate_key_share(n, t, keygen_timestamp)
            .await
            .unwrap()
    });

    let bob_handle = tokio::spawn(async move {
        bob.generate_key_share(n, t, keygen_timestamp)
            .await
            .unwrap()
    });

    let charlie_handle = tokio::spawn(async move {
        charlie
            .generate_key_share(n, t, keygen_timestamp)
            .await
            .unwrap()
    });

    // Wait for all parties concurrently
    let (alice_key, bob_key, charlie_key) = tokio::join!(alice_handle, bob_handle, charlie_handle);
    let alice_key = alice_key.unwrap();
    let bob_key = bob_key.unwrap();
    let charlie_key = charlie_key.unwrap();

    // Phase 3: Sign a message with parties 0 and 1 (threshold is 2)
    tracing::info!("Phase 3: Signing message with parties 0 and 1");
    let message = b"Hello, CGGMP24 threshold cryptography!";
    let signers = vec![0u16, 1u16];

    let signing_coordinator = SigningNetworkCoordinator::new();

    let mut alice = SigningParty::new(0, signing_coordinator.clone(), alice_key.clone()).await;
    let mut bob = SigningParty::new(1, signing_coordinator.clone(), bob_key.clone()).await;

    let msg = message.to_vec();
    let signers_copy = signers.clone();

    let alice_handle = tokio::spawn(async move {
        alice
            .sign_message(&signers_copy, &msg, signing_timestamp)
            .await
    });

    let msg = message.to_vec();
    let signers_copy = signers.clone();

    let bob_handle = tokio::spawn(async move {
        bob.sign_message(&signers_copy, &msg, signing_timestamp)
            .await
    });

    // Wait for all signers concurrently
    let (alice_result, bob_result) = tokio::join!(alice_handle, bob_handle);
    let alice_sig = alice_result.unwrap().expect("Alice signing failed");
    let bob_sig = bob_result.unwrap().expect("Bob signing failed");

    tracing::info!(
        "✅ Signing completed successfully with {} signers",
        signers.len()
    );

    // Verify signatures
    let public_key = &alice_key.shared_public_key;
    let data_to_sign = cggmp24::DataToSign::<Secp256k1>::digest::<Sha256>(message);

    alice_sig
        .verify(public_key, &data_to_sign)
        .expect("Alice signature verification failed");
    tracing::info!("✓ Alice's signature verified successfully");

    bob_sig
        .verify(public_key, &data_to_sign)
        .expect("Bob signature verification failed");
    tracing::info!("✓ Bob's signature verified successfully");

    tracing::info!("✅ All signatures verified successfully!");
}

use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};

#[derive(Clone, Debug)]
pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub generated: bool,
}

pub fn resolve_tls_files(
    custom_cert: Option<&Path>,
    custom_key: Option<&Path>,
    cache_dir: &Path,
    api_ip: &str,
) -> Result<TlsFiles, Box<dyn Error>> {
    match (custom_cert, custom_key) {
        (Some(cert), Some(key)) => {
            require_file(cert, "TLS certificate")?;
            require_file(key, "TLS private key")?;
            return Ok(TlsFiles {
                cert: cert.to_path_buf(),
                key: key.to_path_buf(),
                generated: false,
            });
        }
        (Some(_), None) => {
            return Err("A TLS certificate was provided without a TLS private key".into());
        }
        (None, Some(_)) => {
            return Err("A TLS private key was provided without a TLS certificate".into());
        }
        (None, None) => {}
    }

    let tls_dir = cache_dir.join("tls");
    let cert = tls_dir.join("cert.pem");
    let key = tls_dir.join("key.pem");
    let cert_exists = cert.is_file();
    let key_exists = key.is_file();

    if cert_exists && key_exists {
        return Ok(TlsFiles {
            cert,
            key,
            generated: false,
        });
    }
    if cert_exists != key_exists {
        return Err(format!(
            "The automatic TLS directory {} contains only one of cert.pem/key.pem; provide both or remove the incomplete pair",
            tls_dir.display()
        )
        .into());
    }

    generate_self_signed_cert(&cert, &key, api_ip)?;
    Ok(TlsFiles {
        cert,
        key,
        generated: true,
    })
}

fn require_file(path: &Path, label: &str) -> Result<(), Box<dyn Error>> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{label} file does not exist: {}", path.display()).into())
    }
}

fn generate_self_signed_cert(
    cert_path: &Path,
    key_path: &Path,
    api_ip: &str,
) -> Result<(), Box<dyn Error>> {
    let parent = cert_path
        .parent()
        .ok_or("Automatic TLS certificate path has no parent directory")?;
    std::fs::create_dir_all(parent)?;

    let suffix = format!("{}-{:016x}", std::process::id(), rand::random::<u64>());
    let cert_tmp = parent.join(format!(".cert.pem.{suffix}.tmp"));
    let key_tmp = parent.join(format!(".key.pem.{suffix}.tmp"));

    let mut subject_alt_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    if let Ok(ip) = api_ip.parse::<IpAddr>() {
        if !ip.is_unspecified() && !ip.is_loopback() {
            subject_alt_names.push(ip.to_string());
        }
    }
    println!(
        "[TLS] Generating persistent self-signed certificate at {}",
        cert_path.display()
    );
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::new(subject_alt_names)?;
    params.not_before = now - Duration::minutes(5);
    params.not_after = now + Duration::days(3650);
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate()?;
    let certificate = params.self_signed(&key)?;

    let mut key_options = OpenOptions::new();
    key_options.write(true).create_new(true);
    let mut cert_options = OpenOptions::new();
    cert_options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Restrict the private key from its first write, regardless of umask.
        key_options.mode(0o600);
        cert_options.mode(0o644);
    }

    let result = (|| -> std::io::Result<()> {
        key_options
            .open(&key_tmp)?
            .write_all(key.serialize_pem().as_bytes())?;
        cert_options
            .open(&cert_tmp)?
            .write_all(certificate.pem().as_bytes())?;
        std::fs::rename(&cert_tmp, cert_path)?;
        std::fs::rename(&key_tmp, key_path)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&cert_tmp);
        let _ = std::fs::remove_file(&key_tmp);
        return Err(format!(
            "Failed to save the HTTPS certificate and key in {}: {error}",
            parent.display()
        )
        .into());
    }
    println!("[TLS] Self-signed certificate generated successfully.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_tls_files;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(test_name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "spotifm-tls-{test_name}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn accepts_matching_custom_certificate_and_key() {
        let temp = TempDir::new("custom-pair");
        let cert = temp.0.join("fullchain.pem");
        let key = temp.0.join("privkey.pem");
        fs::write(&cert, "certificate").unwrap();
        fs::write(&key, "key").unwrap();

        let files = resolve_tls_files(Some(&cert), Some(&key), &temp.0, "0.0.0.0").unwrap();
        assert_eq!(files.cert, cert);
        assert_eq!(files.key, key);
        assert!(!files.generated);
    }

    #[test]
    fn rejects_incomplete_custom_certificate_pair() {
        let temp = TempDir::new("incomplete-custom-pair");
        let cert = temp.0.join("fullchain.pem");
        fs::write(&cert, "certificate").unwrap();

        let error = resolve_tls_files(Some(&cert), None, &temp.0, "0.0.0.0").unwrap_err();
        assert!(error.to_string().contains("without a TLS private key"));
    }

    #[test]
    fn reuses_pair_from_automatic_tls_directory() {
        let temp = TempDir::new("automatic-pair");
        let tls_dir = temp.0.join("tls");
        let cert = tls_dir.join("cert.pem");
        let key = tls_dir.join("key.pem");
        fs::create_dir_all(&tls_dir).unwrap();
        fs::write(&cert, "certificate").unwrap();
        fs::write(&key, "key").unwrap();

        let files = resolve_tls_files(None, None, &temp.0, "0.0.0.0").unwrap();
        assert_eq!(files.cert, cert);
        assert_eq!(files.key, key);
        assert!(!files.generated);
    }

    #[test]
    fn generates_valid_self_signed_certificate_when_pair_is_absent() {
        use rustls::client::danger::ServerCertVerifier;
        use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName, UnixTime};
        use std::sync::Arc;

        let temp = TempDir::new("generate-pair");
        let files = resolve_tls_files(None, None, &temp.0, "192.0.2.10").unwrap();

        assert!(files.generated);
        assert!(files.cert.is_file());
        assert!(files.key.is_file());

        let cert = CertificateDer::from_pem_file(&files.cert).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.clone()).unwrap();
        let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots),
            Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .unwrap();
        for name in ["localhost", "127.0.0.1", "::1", "192.0.2.10"] {
            verifier
                .verify_server_cert(
                    &cert,
                    &[],
                    &ServerName::try_from(name).unwrap(),
                    &[],
                    UnixTime::now(),
                )
                .unwrap();
        }
        assert!(verifier
            .verify_server_cert(
                &cert,
                &[],
                &ServerName::try_from("192.0.2.11").unwrap(),
                &[],
                UnixTime::now(),
            )
            .is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&files.key).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let cert_contents = fs::read(&files.cert).unwrap();
        let key_contents = fs::read(&files.key).unwrap();
        let reused = resolve_tls_files(None, None, &temp.0, "192.0.2.10").unwrap();
        assert!(!reused.generated);
        assert_eq!(fs::read(reused.cert).unwrap(), cert_contents);
        assert_eq!(fs::read(reused.key).unwrap(), key_contents);
    }

    #[test]
    fn failed_certificate_write_cleans_up_temporary_key() {
        let temp = TempDir::new("failed-write");
        let tls_dir = temp.0.join("tls");
        fs::create_dir_all(tls_dir.join("cert.pem")).unwrap();
        let error = resolve_tls_files(None, None, &temp.0, "0.0.0.0").unwrap_err();
        assert!(error
            .to_string()
            .contains("Failed to save the HTTPS certificate"));
        let remaining: Vec<_> = fs::read_dir(&tls_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(remaining, ["cert.pem"]);
    }

    #[tokio::test]
    async fn generated_pair_serves_http_and_https() {
        use warp::Filter;

        let temp = TempDir::new("https-listener");
        let files = resolve_tls_files(None, None, &temp.0, "127.0.0.1").unwrap();
        let route = warp::path::end().map(|| "spotifm tls");
        let (http_address, http_server) = warp::serve(route).bind_ephemeral(([127, 0, 0, 1], 0));
        let (https_address, https_server) = warp::serve(route)
            .tls()
            .cert_path(&files.cert)
            .key_path(&files.key)
            .bind_ephemeral(([127, 0, 0, 1], 0));
        let http_task = tokio::spawn(http_server);
        let https_task = tokio::spawn(https_server);

        let client = reqwest::Client::builder()
            .add_root_certificate(
                reqwest::Certificate::from_pem(&fs::read(&files.cert).unwrap()).unwrap(),
            )
            .build()
            .unwrap();
        let http_response = client
            .get(format!("http://{http_address}/"))
            .send()
            .await
            .unwrap();
        let https_response = client
            .get(format!("https://{https_address}/"))
            .send()
            .await
            .unwrap();

        assert_eq!(http_response.status(), reqwest::StatusCode::OK);
        assert_eq!(http_response.text().await.unwrap(), "spotifm tls");
        assert_eq!(https_response.status(), reqwest::StatusCode::OK);
        assert_eq!(https_response.text().await.unwrap(), "spotifm tls");
        http_task.abort();
        https_task.abort();
    }
}

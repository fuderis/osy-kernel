#[macro_export]
macro_rules! macos_protection {
    () => {{
        #[cfg(target_os = "macos")]
        {
            ::atoman::spawn(async {
                use ::atoman::io::AsyncReadExt;
                let mut std_in = atoman::io::stdin();
                let mut buf = [0; 1];
                if let Ok(0) = std_in.read(&mut buf).await {
                    ::std::process::exit(0);
                }
            });
        }
    }};
}

#[cfg(unix)]
#[macro_export]
macro_rules! has_sudo_priv {
    () => {
        ::atoman::process::Command::new("sudo")
            .args(["-n", "true"])
            .status()
            .await
            .map(|status| status.success())
            .unwrap_or(false)
    };
}

#[cfg(unix)]
#[macro_export]
macro_rules! ensure_sudo_priv {
    () => {{
        if !::osy_share::has_sudo_priv!() {
            return Err(Error::Custom(str!(
                "Sudo privileges are required to perform this operation."
            ))
            .into());
        }
        Result::<()>::Ok(())
    }};
}

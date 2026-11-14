// Temporary: one pairing try on a real accessory, the code typed once it shows, then Breezone's own
// pairing is removed.
use breezone_server::homekit::{remove_own_pairing, session::Connection, PairSetup};
use std::time::{Duration, Instant};

#[tokio::test]
#[ignore]
async fn try_pairing() {
    let addr = std::env::var("HK_ADDR").unwrap();
    let code_file = std::env::var("HK_CODE_FILE").unwrap();
    let _ = std::fs::remove_file(&code_file);
    let hosts: Vec<std::net::Ipv4Addr> = (1..255).map(|i| std::net::Ipv4Addr::new(192, 168, 86, i)).collect();
    let found: Vec<_> = breezone_server::homekit::discovery::browse(Duration::from_secs(3), &hosts).await.into_iter().filter(|a| a.host == addr).collect();
    println!("ANNOUNCED {found:?}");
    let auth = match std::env::var("HK_AUTH").ok().as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => found.iter().find(|a| a.host == addr).is_some_and(|a| a.flags & 1 == 1),
    };
    let pairing = if let Ok(fixed) = std::env::var("HK_CODE") {
        // A printed code: each SRP variant tried in turn, on a new pairing each time.
        let code = breezone_server::homekit::setup_code(&fixed).unwrap();
        let mut paired = None;
        for variant in std::env::var("HK_VARIANTS").unwrap_or_else(|_| "0,1,2,3".into()).split(',') {
            std::env::set_var("BREEZONE_SRP_VARIANT", variant);
            println!("TRY variant={variant} with_auth={auth}");
            match PairSetup::start(&addr, auth).await {
                Ok(setup) => match setup.finish(&code).await {
                    Ok(p) => {
                        println!("VARIANT {variant} WORKS");
                        paired = Some(p);
                        break;
                    }
                    Err(e) => println!("VARIANT {variant} FAILED {e:?}"),
                },
                Err(e) => println!("START FAILED {e:?}"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        paired.expect("no variant works")
    } else {
        println!("START with_auth={auth}");
        let setup = match PairSetup::start(&addr, auth).await {
            Ok(s) => s,
            Err(e) => panic!("START FAILED {e:?}"),
        };
        println!("WAITING for the code in {code_file}");
        let since = Instant::now();
        let code = loop {
            if let Ok(c) = std::fs::read_to_string(&code_file) {
                if let Some(c) = breezone_server::homekit::setup_code(c.trim()) {
                    break c;
                }
            }
            assert!(since.elapsed() < Duration::from_secs(600), "no code typed");
            tokio::time::sleep(Duration::from_millis(300)).await;
        };
        println!("CODE {code} after {:?}", since.elapsed());
        match setup.finish(&code).await {
            Ok(p) => p,
            Err(e) => panic!("FINISH FAILED {e:?}"),
        }
    };
    println!("PAIRED {}", pairing.accessory_id);
    match Connection::verify(&addr, &pairing).await {
        Ok(mut c) => match c.get_json("/accessories").await {
            Ok(db) => std::fs::write(std::env::var("HK_OUT").unwrap(), serde_json::to_string_pretty(&db).unwrap()).unwrap(),
            Err(e) => println!("ACCESSORIES FAILED {e:?}"),
        },
        Err(e) => println!("VERIFY FAILED {e:?}"),
    }
    match remove_own_pairing(&addr, &pairing).await {
        Ok(()) => println!("REMOVED own pairing"),
        Err(e) => println!("REMOVE FAILED {e:?} pairing={}", serde_json::to_string(&pairing).unwrap()),
    }
}

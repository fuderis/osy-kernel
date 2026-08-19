use crate::prelude::*;
use anylm::api::Tool;
use system_utils::SystemMonitor;

static SYSTEM_MONITOR: State<SystemMonitor> = State::default();

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //              BASIC INFO
        Tool::new(
            "get_system_info",
            "Returns static system information including operating system, CPU, GPU, RAM, motherboard, storage devices, and other hardware details.",
        ),
        // ________________________________________
        //              SYSTEM METRICS
        Tool::new(
            "get_system_metrics",
            "Returns current live system metrics including CPU usage, memory usage, temperatures, disk usage, network activity and other runtime statistics.",
        ),
        // ________________________________________
        //              DEVICES LIST
        Tool::new(
            "get_devices_list",
            "Returns a formatted list of currently connected hardware devices.",
        ),
    ]
}

#[log(skip_all)]
pub async fn handle_system_info(tx: Sender<Bytes>, _payload: ()) -> Result<()> {
    let info = SYSTEM_MONITOR.lock().await.info();
    let msg = str!(info);

    tx.send(Event::answer(msg))?;
    Ok(())
}

#[log(skip_all)]
pub async fn handle_system_metrics(tx: Sender<Bytes>, _payload: ()) -> Result<()> {
    let metrics = SYSTEM_MONITOR
        .lock()
        .await
        .refresh_metrics_with_interval(Duration::from_secs(10));
    let msg = str!(metrics);

    info!("System metrics collected.");
    tx.send(Event::answer(msg))?;
    Ok(())
}

#[log(skip_all)]
pub async fn handle_devices_list(tx: Sender<Bytes>, _payload: ()) -> Result<()> {
    let devices = SYSTEM_MONITOR
        .lock()
        .await
        .refresh_devices_with_interval(Duration::from_secs(60));
    let msg = str!(devices);

    info!("Connected devices enumerated.");
    tx.send(Event::answer(msg))?;
    Ok(())
}

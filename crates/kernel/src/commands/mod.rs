pub mod chat;
pub mod health;
pub mod server;

use rigging::Stylize;

pub fn section(title: &str) {
    println!();
    println!("{}", title.bold());
}

pub fn info(label: &str, message: &str) {
    if !label.is_empty() {
        println!("  • {}{} {}", label.bold(), ":".bold(), message);
    } else {
        println!("  • {}", message);
    }
}

pub fn item(label: &str, message: &str) {
    let message = message.replace('\n', "\n     ");

    if !label.is_empty() {
        println!("  └─ {}{} {}", label.bold(), ":".bold(), message);
    } else {
        println!("  └─ {}", message);
    }
}

pub fn warn(message: &str) {
    if let Some((prefix, tail)) = message.split_once(": ") {
        println!(
            "  {} {}{} {tail}",
            "ℹ".yellow(),
            prefix.yellow(),
            ":".yellow()
        );
    } else {
        println!("  {} {}", "ℹ".yellow(), message);
    }
}

pub fn success(message: &str) {
    println!("  {} {}", "✓".green(), message);
}

pub fn error(e: crate::DynError) {
    if let Some((prefix, tail)) = crate::str!(e).split_once(": ") {
        println!("\n  {} {}{} {tail}", "✗".red(), prefix.red(), ":".red());
    } else {
        println!("\n  {} {e}", "✗".red());
    }
}

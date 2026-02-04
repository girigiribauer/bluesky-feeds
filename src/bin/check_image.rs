use anyhow::{Context, Result};
use fakebluesky::image_analyzer::{analyze_image, perform_analysis, BlueDetectionConfig};
use std::env;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: check_image <image_path_or_url>");
        std::process::exit(1);
    }

    let target = &args[1];
    println!("Analyzing: {}", target);

    let config = BlueDetectionConfig::default();

    // Check if target is URL
    let result = if target.starts_with("http://") || target.starts_with("https://") {
        analyze_image(target, &config).await?
    } else {
        // Assume local file
        let path = Path::new(target);
        if !path.exists() {
             eprintln!("File not found: {}", target);
             std::process::exit(1);
        }
        let img = image::open(path).context("Failed to open local image")?;
        perform_analysis(&img, &config)
    };

    println!("----------------------------------------");
    println!("Blue Score: {:.2}", result.score);
    println!("Is Blue Sky: {}", result.is_blue_sky);
    println!("----------------------------------------");
    println!("Total Pixels (Top 30%): {}", result.total_pixels);
    println!("Blue Pixels: {}", result.blue_pixels);
    println!("Threshold: {:.2}", config.blue_threshold);
    println!("----------------------------------------");

    if result.is_blue_sky {
        println!("Result: REJECTED (Contains Blue Sky)");
    } else {
        println!("Result: ACCEPTED (Not Blue Sky)");
    }

    Ok(())
}


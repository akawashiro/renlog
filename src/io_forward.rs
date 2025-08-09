use std::fs::File;
use std::io::{self, Read, Result, Write};
use tokio::fs::File as AsyncFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BUFFER_SIZE: usize = 32 * 1024;

// Simple synchronous forwarding from input to output
pub fn forward_simple(mut input: File, mut output: File) -> Result<()> {
    let mut buffer = [0u8; BUFFER_SIZE];

    loop {
        match input.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                output.write_all(&buffer[..n])?;
                output.flush()?;
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

// Tee functionality - read from input and write to both outputs
pub fn tee_simple(mut input: File, mut output1: File, mut output2: File) -> Result<()> {
    let mut buffer = [0u8; BUFFER_SIZE];

    loop {
        match input.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                let data = &buffer[..n];

                // Write to first output (typically stdout)
                if let Err(e) = output1.write_all(data) {
                    eprintln!("Warning: Failed to write to output1: {}", e);
                }
                if let Err(e) = output1.flush() {
                    eprintln!("Warning: Failed to flush output1: {}", e);
                }

                // Write to second output (typically logger pipe)
                if let Err(e) = output2.write_all(data) {
                    eprintln!("Warning: Failed to write to output2: {}", e);
                }
                if let Err(e) = output2.flush() {
                    eprintln!("Warning: Failed to flush output2: {}", e);
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

// Async version of forward for use with tokio
pub async fn forward_async(mut input: AsyncFile, mut output: AsyncFile) -> Result<()> {
    let mut buffer = [0u8; BUFFER_SIZE];

    loop {
        match input.read(&mut buffer).await {
            Ok(0) => break, // EOF
            Ok(n) => {
                output.write_all(&buffer[..n]).await?;
                output.flush().await?;
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

// Async version of tee for use with tokio
pub async fn tee_async(
    mut input: AsyncFile,
    mut output1: AsyncFile,
    mut output2: AsyncFile,
) -> Result<()> {
    let mut buffer = [0u8; BUFFER_SIZE];

    loop {
        match input.read(&mut buffer).await {
            Ok(0) => break, // EOF
            Ok(n) => {
                let data = &buffer[..n];

                // Write to first output (typically stdout)
                if let Err(e) = output1.write_all(data).await {
                    eprintln!("Warning: Failed to write to output1: {}", e);
                }
                if let Err(e) = output1.flush().await {
                    eprintln!("Warning: Failed to flush output1: {}", e);
                }

                // Write to second output (typically logger pipe)
                if let Err(e) = output2.write_all(data).await {
                    eprintln!("Warning: Failed to write to output2: {}", e);
                }
                if let Err(e) = output2.flush().await {
                    eprintln!("Warning: Failed to flush output2: {}", e);
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

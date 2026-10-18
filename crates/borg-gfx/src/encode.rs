use base64::{Engine, engine::general_purpose::STANDARD};

use crate::{Fit, GfxError, Picture};

const KITTY_CHUNK: usize = 4096;
const SAVE_CURSOR: &str = "\x1b7";
const RESTORE_CURSOR: &str = "\x1b8";

pub fn at(fit: &Fit, body: &str) -> String {
    format!("{SAVE_CURSOR}\x1b[{};{}H{body}{RESTORE_CURSOR}", fit.area.y + 1, fit.area.x + 1)
}

pub fn kitty_transmit(picture: &Picture, image: u32) -> Result<String, GfxError> {
    let data = STANDARD.encode(picture.png()?);
    let chunks: Vec<&str> = data.as_bytes().chunks(KITTY_CHUNK).filter_map(|c| std::str::from_utf8(c).ok()).collect();
    let last = chunks.len().saturating_sub(1);
    Ok(chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let more = u8::from(index < last);
            if index == 0 {
                format!("\x1b_Ga=t,f=100,t=d,i={image},q=2,m={more};{chunk}\x1b\\")
            } else {
                format!("\x1b_Gm={more};{chunk}\x1b\\")
            }
        })
        .collect())
}

pub fn kitty_place(fit: &Fit, image: u32, placement: u32, z: i32) -> String {
    let (x, y) = fit.offset;
    let body = format!(
        "\x1b_Ga=p,i={image},p={placement},c={},r={},X={x},Y={y},C=1,z={z},q=2\x1b\\",
        fit.area.width, fit.area.height
    );
    at(fit, &body)
}

pub fn kitty_remove(image: u32, placement: u32) -> String {
    format!("\x1b_Ga=d,d=i,i={image},p={placement},q=2\x1b\\")
}

pub fn kitty_forget(image: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={image},q=2\x1b\\")
}

pub fn iterm(picture: &Picture, fit: &Fit) -> Result<String, GfxError> {
    let png = picture.png()?;
    let (width, height) = fit.size;
    let body = format!(
        "\x1b]1337;File=inline=1;size={};width={width}px;height={height}px;preserveAspectRatio=1;doNotMoveCursor=1:{}\x07",
        png.len(),
        STANDARD.encode(png)
    );
    Ok(at(fit, &body))
}

pub fn sixel(picture: &Picture, fit: &Fit) -> Result<String, GfxError> {
    let (width, height) = fit.size;
    let scaled = picture.resized(width, height)?;
    let to_usize = |value: u32| usize::try_from(value).map_err(|_| GfxError::TooLarge);
    let body = icy_sixel::SixelImage::try_from_rgba(scaled.rgba().to_vec(), to_usize(width)?, to_usize(height)?)
        .and_then(|image| image.encode())
        .map_err(|e| GfxError::Encode(e.to_string()))?;
    Ok(at(fit, &body))
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::*;

    fn dot() -> Picture {
        Picture::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap_or_else(|e| panic!("{e}"))
    }

    const FIT: Fit = Fit { area: Rect { x: 4, y: 2, width: 3, height: 1 }, offset: (1, 2), size: (1, 1) };

    #[test]
    fn kitty_commands_address_cells_and_images() {
        let transmit = kitty_transmit(&dot(), 7).unwrap_or_else(|e| panic!("{e}"));
        assert!(transmit.starts_with("\x1b_Ga=t,f=100,t=d,i=7,q=2,m=0;"));
        assert_eq!(
            kitty_place(&FIT, 7, 3, -1),
            "\x1b7\x1b[3;5H\x1b_Ga=p,i=7,p=3,c=3,r=1,X=1,Y=2,C=1,z=-1,q=2\x1b\\\x1b8"
        );
        assert_eq!(kitty_remove(7, 3), "\x1b_Ga=d,d=i,i=7,p=3,q=2\x1b\\");
    }

    #[test]
    fn large_payloads_are_chunked() {
        let noisy: Vec<u8> = (0..64u32 * 64).flat_map(|i| (i.wrapping_mul(2_654_435_761)).to_le_bytes()).collect();
        let picture = Picture::from_rgba(64, 64, noisy).unwrap_or_else(|e| panic!("{e}"));
        let transmit = kitty_transmit(&picture, 1).unwrap_or_else(|e| panic!("{e}"));
        assert!(transmit.matches("\x1b_G").count() > 1);
        assert!(transmit.contains("m=1;") && transmit.contains("\x1b_Gm=0;"));
    }

    #[test]
    fn inline_protocols_encode_payloads() {
        assert!(iterm(&dot(), &FIT).unwrap_or_else(|e| panic!("{e}")).contains("\x1b]1337;File=inline=1;"));
        let sixel = sixel(&dot(), &FIT).unwrap_or_else(|e| panic!("{e}"));
        assert!(sixel.contains("\x1bP") && sixel.ends_with("\x1b8"));
    }
}

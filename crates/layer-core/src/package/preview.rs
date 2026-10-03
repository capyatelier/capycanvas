use std::{io::Cursor, sync::Arc};

pub const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PREVIEW_DIMENSION: u32 = 1024;
#[derive(Clone, Debug)]
pub struct Preview { size: [u32; 2], encoded: Arc<[u8]>, pixels: Arc<[u8]> }
impl Preview {
    pub fn size(&self) -> [u32; 2] { self.size }
    pub fn encoded(&self) -> &Arc<[u8]> { &self.encoded }
    pub fn pixels(&self) -> &Arc<[u8]> { &self.pixels }
    pub fn decode(encoded: Arc<[u8]>) -> Result<Self, String> {
        if encoded.len() > MAX_PREVIEW_BYTES || !encoded.starts_with(b"\x89PNG\r\n\x1a\n") { return Err("Invalid preview envelope".into()); }
        let mut offset = 8usize;
        let mut end = false;
        while offset < encoded.len() {
            let header=encoded.get(offset..offset.checked_add(8).ok_or("Preview chunk overflow")?).ok_or("Incomplete preview chunk")?;
            let length=u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
            let tail=offset.checked_add(12).and_then(|start|start.checked_add(length)).ok_or("Preview chunk overflow")?;
            let chunk=encoded.get(offset..tail).ok_or("Incomplete preview chunk")?;
            let kind=&header[4..8];
            if crc32fast::hash(&chunk[4..8+length])!=u32::from_be_bytes(chunk[8+length..].try_into().unwrap()) {return Err("Preview checksum failed".into());}
            if matches!(kind,b"iCCP"|b"cICP"|b"acTL"|b"fcTL"|b"fdAT") {return Err("Unsupported preview interpretation".into());}
            offset=tail;
            if kind==b"IEND" { if length!=0 || offset!=encoded.len() {return Err("Invalid preview tail".into());} end=true; break; }
        }
        if !end {return Err("Missing preview tail".into());}
        let mut decoder=png::Decoder::new(Cursor::new(&*encoded));
        decoder.set_limits(png::Limits {bytes:MAX_PREVIEW_BYTES});
        decoder.set_ignore_text_chunk(true);
        let mut reader=decoder.read_info().map_err(|e|e.to_string())?;
        let info=reader.info();
        let size=[info.width,info.height];
        if size.contains(&0) || size.iter().any(|n|*n>MAX_PREVIEW_DIMENSION) || info.bit_depth!=png::BitDepth::Eight || info.color_type!=png::ColorType::Rgba || info.interlaced || info.srgb.is_none() {
            return Err("Unsupported preview representation".into());
        }
        let mut pixels=vec![0;size[0] as usize*size[1] as usize*4];
        let output=reader.next_frame(&mut pixels).map_err(|e|e.to_string())?;
        if output.buffer_size()!=pixels.len() {return Err("Invalid decoded preview length".into());}
        reader.finish().map_err(|e|e.to_string())?;
        Ok(Self {size,encoded,pixels:pixels.into()})
    }
    pub fn from_rgba(size:[u32;2],pixels:Arc<[u8]>) -> Result<Self,String> {
        if size.contains(&0) || size.iter().any(|n|*n>MAX_PREVIEW_DIMENSION) || pixels.len()!=size[0] as usize*size[1] as usize*4 {return Err("Invalid preview pixels".into());}
        let mut bytes=Vec::new();
        {
            let mut encoder=png::Encoder::new(&mut bytes,size[0],size[1]);
            encoder.set_color(png::ColorType::Rgba); encoder.set_depth(png::BitDepth::Eight);
            encoder.set_source_srgb(png::SrgbRenderingIntent::RelativeColorimetric);
            let mut writer=encoder.write_header().map_err(|e|e.to_string())?;
            writer.write_image_data(&pixels).map_err(|e|e.to_string())?;
            writer.finish().map_err(|e|e.to_string())?;
        }
        if bytes.len()>MAX_PREVIEW_BYTES {return Err("Preview exceeds encoded bound".into());}
        Ok(Self {size,encoded:bytes.into(),pixels})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_preview_is_exact_and_rejects_corruption_and_unbounded_shapes() {
        let pixels:Arc<[u8]>=[255,12,3,0,0,19,255,128].into();
        let preview=Preview::from_rgba([2,1],pixels.clone()).unwrap();
        let reopened=Preview::decode(preview.encoded().clone()).unwrap();
        assert_eq!(reopened.size(),[2,1]); assert_eq!(reopened.pixels(),&pixels);
        let mut broken=preview.encoded().to_vec();broken[25]^=1;assert!(Preview::decode(broken.into()).is_err());
        let mut extra=preview.encoded().to_vec();extra.push(0);assert!(Preview::decode(extra.into()).is_err());
        assert!(Preview::decode(preview.encoded()[..preview.encoded().len()-1].into()).is_err());
        assert!(Preview::from_rgba([1025,1],vec![0;4100].into()).is_err());
    }
}

use anyhow::{Context, bail};

use crate::{
    file_ref::decode_file_reference,
    io_util::{read_u16, read_u32},
};

#[derive(Debug, Clone)]
pub(crate) struct PeSection {
    pub(crate) name: String,
    pub(crate) virtual_address: u32,
    pub(crate) virtual_size: u32,
    pub(crate) raw_pointer: u32,
    pub(crate) raw_size: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct PeImage {
    image_base: u32,
    sections: Vec<PeSection>,
    data_len: usize,
}

impl PeImage {
    pub(crate) fn parse(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < 0x40 {
            bail!("PE file too small");
        }

        let pe_offset = read_u32(data, 0x3c)? as usize;
        let signature_end = pe_offset
            .checked_add(4)
            .context("PE signature offset overflow")?;
        if data.get(pe_offset..signature_end) != Some(b"PE\0\0".as_slice()) {
            bail!("invalid PE signature");
        }

        let coff_offset = pe_offset
            .checked_add(4)
            .context("PE COFF offset overflow")?;
        let optional_offset = coff_offset
            .checked_add(20)
            .context("PE optional header offset overflow")?;
        data.get(coff_offset..optional_offset)
            .context("PE COFF header truncated")?;

        let section_count = read_u16(data, coff_offset + 2)? as usize;
        let optional_header_size = read_u16(data, coff_offset + 16)? as usize;
        let image_base = read_u32(data, optional_offset + 28)?;
        let section_offset = optional_offset
            .checked_add(optional_header_size)
            .context("PE section table offset overflow")?;
        let section_table_bytes = section_count
            .checked_mul(40)
            .context("PE section table size overflow")?;
        let section_table_end = section_offset
            .checked_add(section_table_bytes)
            .context("PE section table end overflow")?;
        data.get(section_offset..section_table_end)
            .context("PE section table truncated")?;

        let mut sections = Vec::with_capacity(section_count);
        for index in 0..section_count {
            let offset = section_offset + index * 40;
            let mut name_bytes = [0_u8; 8];
            name_bytes.copy_from_slice(
                data.get(offset..offset + 8)
                    .context("PE section name truncated")?,
            );
            let name = name_bytes
                .split(|&byte| byte == 0)
                .next()
                .unwrap_or(&name_bytes);
            let virtual_size = read_u32(data, offset + 8)?;
            let virtual_address = read_u32(data, offset + 12)?;
            let raw_size = read_u32(data, offset + 16)?;
            let raw_pointer = read_u32(data, offset + 20)?;
            let raw_end = u64::from(raw_pointer)
                .checked_add(u64::from(raw_size))
                .context("PE section raw range overflow")?;
            if raw_end > data.len() as u64 {
                bail!(
                    "PE section {} raw range {}..{} exceeds file size {}",
                    String::from_utf8_lossy(name),
                    raw_pointer,
                    raw_end,
                    data.len()
                );
            }

            sections.push(PeSection {
                name: String::from_utf8_lossy(name).into_owned(),
                virtual_address,
                virtual_size: virtual_size.max(raw_size),
                raw_pointer,
                raw_size,
            });
        }

        Ok(Self {
            image_base,
            sections,
            data_len: data.len(),
        })
    }

    pub(crate) fn sections(&self) -> &[PeSection] {
        &self.sections
    }

    pub(crate) fn va_to_file_offset(&self, va: u32) -> anyhow::Result<usize> {
        let rva = va.checked_sub(self.image_base).with_context(|| {
            format!("PE VA 0x{va:x} is below image base 0x{:x}", self.image_base)
        })?;
        let rva = u64::from(rva);

        for section in &self.sections {
            let start = u64::from(section.virtual_address);
            let end = start
                .checked_add(u64::from(section.virtual_size))
                .context("PE section virtual range overflow")?;
            if !(start..end).contains(&rva) {
                continue;
            }

            let delta = rva - start;
            if delta >= u64::from(section.raw_size) {
                bail!("PE VA 0x{va:x} points past raw section data");
            }
            let raw_offset = u64::from(section.raw_pointer)
                .checked_add(delta)
                .context("PE raw offset overflow")?;
            if raw_offset > self.data_len as u64 {
                bail!("PE VA 0x{va:x} maps past file size");
            }
            return usize::try_from(raw_offset).context("PE raw offset exceeds usize");
        }

        bail!("PE VA 0x{va:x} not covered by any section")
    }

    pub(crate) fn read_u32_at(
        &self,
        data: &[u8],
        offset: usize,
        context: &str,
    ) -> anyhow::Result<u32> {
        let end = offset
            .checked_add(4)
            .with_context(|| format!("{context} offset overflow"))?;
        data.get(offset..end)
            .with_context(|| format!("{context} out of bounds at byte offset {offset}"))?;
        read_u32(data, offset)
    }

    pub(crate) fn read_u32_va(&self, data: &[u8], va: u32, context: &str) -> anyhow::Result<u32> {
        let offset = self.va_to_file_offset(va)?;
        self.read_u32_at(data, offset, context)
    }

    pub(crate) fn language_file_ids(
        &self,
        data: &[u8],
        table_va: u32,
        files_per_language: usize,
        language_index: usize,
    ) -> anyhow::Result<Vec<Option<u32>>> {
        let table_offset = self.va_to_file_offset(table_va)?;
        let row_bytes = language_index
            .checked_mul(files_per_language)
            .and_then(|value| value.checked_mul(4))
            .context("PE language row offset overflow")?;
        let row_offset = table_offset
            .checked_add(row_bytes)
            .context("PE language row offset overflow")?;
        let mut file_ids = Vec::with_capacity(files_per_language);
        for index in 0..files_per_language {
            let ptr_offset = row_offset
                .checked_add(
                    index
                        .checked_mul(4)
                        .context("PE language file index overflow")?,
                )
                .context("PE language pointer offset overflow")?;
            let ptr = self.read_u32_at(
                data,
                ptr_offset,
                &format!("language {language_index} text-file pointer {index}"),
            )?;
            if ptr == 0 {
                file_ids.push(None);
                continue;
            }
            let raw_ref = self.read_u32_va(
                data,
                ptr,
                &format!("language {language_index} text-file reference {index}"),
            )?;
            file_ids.push(Some(decode_file_reference(
                (raw_ref & 0xffff) as u16,
                ((raw_ref >> 16) & 0xffff) as u16,
            )));
        }

        Ok(file_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::PeImage;
    use crate::file_ref::encode_file_reference;

    const IMAGE_BASE: u32 = 0x0040_0000;
    const SECTION_VA: u32 = 0x1000;
    const SECTION_RAW_OFFSET: usize = 0x300;

    fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn test_pe() -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x500];
        let pe_offset = 0x80;
        let coff_offset = pe_offset + 4;
        let optional_offset = coff_offset + 20;
        let optional_size = 0xe0;
        let section_offset = optional_offset + optional_size;

        write_u32(&mut bytes, 0x3c, pe_offset as u32);
        bytes[pe_offset..pe_offset + 4].copy_from_slice(b"PE\0\0");
        write_u16(&mut bytes, coff_offset + 2, 1);
        write_u16(&mut bytes, coff_offset + 16, optional_size as u16);
        write_u16(&mut bytes, optional_offset, 0x010b);
        write_u32(&mut bytes, optional_offset + 28, IMAGE_BASE);

        bytes[section_offset..section_offset + 8].copy_from_slice(b".rdata\0\0");
        write_u32(&mut bytes, section_offset + 8, 0x180);
        write_u32(&mut bytes, section_offset + 12, SECTION_VA);
        write_u32(&mut bytes, section_offset + 16, 0x100);
        write_u32(&mut bytes, section_offset + 20, SECTION_RAW_OFFSET as u32);
        bytes
    }

    #[test]
    fn maps_virtual_addresses_and_resolves_language_file_references() -> anyhow::Result<()> {
        let mut bytes = test_pe();
        let reference_file_id = 123_456;
        let reference_va = IMAGE_BASE + SECTION_VA + 0x20;
        let (id0, id1) = encode_file_reference(reference_file_id);

        write_u32(&mut bytes, SECTION_RAW_OFFSET, reference_va);
        write_u32(&mut bytes, SECTION_RAW_OFFSET + 4, 0);
        write_u32(
            &mut bytes,
            SECTION_RAW_OFFSET + 0x20,
            u32::from(id0) | (u32::from(id1) << 16),
        );

        let pe = PeImage::parse(&bytes)?;
        assert_eq!(
            pe.va_to_file_offset(reference_va)?,
            SECTION_RAW_OFFSET + 0x20
        );
        assert_eq!(
            pe.language_file_ids(&bytes, IMAGE_BASE + SECTION_VA, 2, 0)?,
            vec![Some(reference_file_id), None]
        );
        Ok(())
    }

    #[test]
    fn rejects_virtual_addresses_outside_backed_section_data() -> anyhow::Result<()> {
        let bytes = test_pe();
        let pe = PeImage::parse(&bytes)?;

        assert!(pe.va_to_file_offset(IMAGE_BASE - 1).is_err());
        assert!(
            pe.va_to_file_offset(IMAGE_BASE + SECTION_VA + 0x100)
                .is_err()
        );
        assert!(
            pe.va_to_file_offset(IMAGE_BASE + SECTION_VA + 0x180)
                .is_err()
        );
        Ok(())
    }
}

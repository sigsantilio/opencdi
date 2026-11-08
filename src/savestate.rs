use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use m68k::{CpuCore, CpuType};

use crate::bus::{CdiBus, SharedIn4Owner};
use crate::mcd212::VideoMode;
use crate::dvc::mpeg::{Buffer, Frame, Plane, Video};
use crate::dvc::mpeg::video::Motion;

const SAVESTATE_MAGIC: &[u8; 8] = b"OCDIST03";

struct StateWriter {
    buf: Vec<u8>,
}

impl StateWriter {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4 * 1024 * 1024),
        }
    }

    fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn write_bool(&mut self, v: bool) {
        self.buf.push(if v { 1 } else { 0 });
    }

    fn write_u16(&mut self, v: u16) {
        let _ = self.buf.write_u16::<LittleEndian>(v);
    }

    fn write_i16(&mut self, v: i16) {
        let _ = self.buf.write_i16::<LittleEndian>(v);
    }

    fn write_u32(&mut self, v: u32) {
        let _ = self.buf.write_u32::<LittleEndian>(v);
    }

    fn write_i32(&mut self, v: i32) {
        let _ = self.buf.write_i32::<LittleEndian>(v);
    }

    fn write_u64(&mut self, v: u64) {
        let _ = self.buf.write_u64::<LittleEndian>(v);
    }

    fn write_i64(&mut self, v: i64) {
        let _ = self.buf.write_i64::<LittleEndian>(v);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    fn write_f32(&mut self, v: f32) {
        let _ = self.buf.write_f32::<LittleEndian>(v);
    }

    fn write_f64(&mut self, v: f64) {
        let _ = self.buf.write_f64::<LittleEndian>(v);
    }

    fn write_bytes(&mut self, slice: &[u8]) {
        self.write_u64(slice.len() as u64);
        self.buf.extend_from_slice(slice);
    }

    fn write_fixed_bytes(&mut self, slice: &[u8]) {
        self.buf.extend_from_slice(slice);
    }

    fn write_opt_u64(&mut self, opt: Option<u64>) {
        match opt {
            Some(v) => {
                self.write_bool(true);
                self.write_u64(v);
            }
            None => self.write_bool(false),
        }
    }

    fn write_opt_u32(&mut self, opt: Option<u32>) {
        match opt {
            Some(v) => {
                self.write_bool(true);
                self.write_u32(v);
            }
            None => self.write_bool(false),
        }
    }

    fn write_opt_u8(&mut self, opt: Option<u8>) {
        match opt {
            Some(v) => {
                self.write_bool(true);
                self.write_u8(v);
            }
            None => self.write_bool(false),
        }
    }
}

struct StateReader<'a> {
    cursor: io::Cursor<&'a [u8]>,
}

impl<'a> StateReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: io::Cursor::new(data),
        }
    }

    fn read_u8(&mut self) -> io::Result<u8> {
        self.cursor.read_u8()
    }

    fn read_bool(&mut self) -> io::Result<bool> {
        Ok(self.read_u8()? != 0)
    }

    fn read_u16(&mut self) -> io::Result<u16> {
        self.cursor.read_u16::<LittleEndian>()
    }

    fn read_i16(&mut self) -> io::Result<i16> {
        self.cursor.read_i16::<LittleEndian>()
    }

    fn read_u32(&mut self) -> io::Result<u32> {
        self.cursor.read_u32::<LittleEndian>()
    }

    fn read_i32(&mut self) -> io::Result<i32> {
        self.cursor.read_i32::<LittleEndian>()
    }

    fn read_u64(&mut self) -> io::Result<u64> {
        self.cursor.read_u64::<LittleEndian>()
    }

    fn read_i64(&mut self) -> io::Result<i64> {
        self.cursor.read_i64::<LittleEndian>()
    }

    fn read_usize(&mut self) -> io::Result<usize> {
        Ok(self.read_u64()? as usize)
    }

    fn read_f32(&mut self) -> io::Result<f32> {
        self.cursor.read_f32::<LittleEndian>()
    }

    fn read_f64(&mut self) -> io::Result<f64> {
        self.cursor.read_f64::<LittleEndian>()
    }

    fn read_bytes(&mut self) -> io::Result<Vec<u8>> {
        let len = self.read_usize()?;
        if len > 64 * 1024 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Buffer length too large in save state"));
        }
        let mut v = vec![0u8; len];
        self.cursor.read_exact(&mut v)?;
        Ok(v)
    }

    fn read_fixed_bytes(&mut self, dest: &mut [u8]) -> io::Result<()> {
        self.cursor.read_exact(dest)
    }

    fn read_opt_u64(&mut self) -> io::Result<Option<u64>> {
        if self.read_bool()? {
            Ok(Some(self.read_u64()?))
        } else {
            Ok(None)
        }
    }

    fn read_opt_u32(&mut self) -> io::Result<Option<u32>> {
        if self.read_bool()? {
            Ok(Some(self.read_u32()?))
        } else {
            Ok(None)
        }
    }

    fn read_opt_u8(&mut self) -> io::Result<Option<u8>> {
        if self.read_bool()? {
            Ok(Some(self.read_u8()?))
        } else {
            Ok(None)
        }
    }
}

pub fn save_state_to_file<P: AsRef<Path>>(
    path: P,
    cpu: &CpuCore,
    bus: &CdiBus,
    disc_launched: bool,
    shell_ready_count: u32,
    pad_x: i32,
    pad_y: i32,
) -> io::Result<()> {
    let mut w = StateWriter::new();
    w.write_fixed_bytes(SAVESTATE_MAGIC);

    // 1. Emulation frontend state
    w.write_bool(disc_launched);
    w.write_u32(shell_ready_count);
    w.write_i32(pad_x);
    w.write_i32(pad_y);

    // 2. CPU Core state (SCC68070)
    for &r in &cpu.dar { w.write_u32(r); }
    for &r in &cpu.dar_save { w.write_u32(r); }
    w.write_u16(cpu.sr_save);
    w.write_u32(cpu.ppc);
    w.write_u32(cpu.pc);
    for &s in &cpu.sp { w.write_u32(s); }
    w.write_u32(cpu.vbr);
    w.write_u32(cpu.sfc);
    w.write_u32(cpu.dfc);
    w.write_u32(cpu.cacr);
    w.write_u32(cpu.caar);
    w.write_u32(cpu.ir);
    w.write_u32(cpu.t1_flag);
    w.write_u32(cpu.t0_flag);
    w.write_u32(cpu.s_flag);
    w.write_u32(cpu.m_flag);
    w.write_u32(cpu.x_flag);
    w.write_u32(cpu.n_flag);
    w.write_u32(cpu.not_z_flag);
    w.write_u32(cpu.v_flag);
    w.write_u32(cpu.c_flag);
    w.write_u32(cpu.int_mask);
    w.write_u32(cpu.int_level);
    w.write_u32(cpu.stopped);
    w.write_bool(cpu.change_of_flow);
    w.write_bool(cpu.loop_mode);
    w.write_u16(cpu.loop_body_word);
    w.write_u16(cpu.loop_dbcc_word);
    w.write_u16(cpu.prefetch_queue[0]);
    w.write_u16(cpu.prefetch_queue[1]);
    w.write_u8(cpu.prefetch_count);
    w.write_bool(cpu.consume_without_prefetch);
    w.write_u32(cpu.pending_sync_clocks);
    w.write_u32(cpu.instr_mode);
    w.write_u32(cpu.run_mode);
    w.write_bool(cpu.exception_processing);
    w.write_u32(cpu.virq_state);
    w.write_u32(cpu.nmi_pending);
    w.write_i32(cpu.cycles_remaining);
    w.write_i32(cpu.initial_cycles);

    // 3. CdiBus state
    w.write_fixed_bytes(&bus.sys_ram[..]);
    w.write_u8(match bus.shared_in4_owner {
        SharedIn4Owner::Idle => 0,
        SharedIn4Owner::Cdic => 1,
        SharedIn4Owner::Vmpeg => 2,
    });
    w.write_u8(bus.scc_uart_cmd);
    w.write_bool(bus.scc_uart_tx_irq);
    w.write_u32(bus.current_pc);
    w.write_u32(bus.wait_cycles);
    w.write_u32(bus.current_line_cycle);
    w.write_bool(bus.emulate_wait_states);
    w.write_u64(bus.current_frame);

    // 4. SccTimer state
    w.write_u8(bus.scc_timer.tsr);
    w.write_u8(bus.scc_timer.tcr);
    w.write_u16(bus.scc_timer.reload);
    w.write_u16(bus.scc_timer.timer0);
    w.write_u16(bus.scc_timer.timer1);
    w.write_u16(bus.scc_timer.timer2);
    w.write_u8(bus.scc_timer.picr1);
    w.write_u8(bus.scc_timer.picr2);
    w.write_u8(bus.scc_timer.lir);
    w.write_bool(bus.scc_timer.int1_line);
    w.write_bool(bus.scc_timer.timer_irq);
    w.write_u32(bus.scc_timer.cycle_accum);

    // 5. SccDma state
    for ch in &bus.dma.channels {
        w.write_u8(ch.channel_status);
        w.write_u8(ch.channel_error);
        w.write_u8(ch.device_control);
        w.write_u8(ch.operation_control);
        w.write_u8(ch.sequence_control);
        w.write_u8(ch.channel_control);
        w.write_u16(ch.transfer_counter);
        w.write_u32(ch.memory_address_counter);
        w.write_u32(ch.device_address_counter);
    }

    // 6. NVRAM state
    w.write_fixed_bytes(&bus.nvram.ram[..]);

    // 7. SlaveHle state
    for ch in &bus.slave.channels {
        w.write_fixed_bytes(&ch.out_buf);
        w.write_usize(ch.out_count);
        w.write_usize(ch.out_index);
        w.write_u8(ch.out_cmd);
        w.write_fixed_bytes(&ch.in_buf);
        w.write_usize(ch.in_count);
        w.write_usize(ch.in_index);
        w.write_fixed_bytes(&ch.pending_buf);
        w.write_usize(ch.pending_count);
        w.write_u8(ch.pending_cmd);
    }
    w.write_i32(bus.slave.device_mouse_x);
    w.write_i32(bus.slave.device_mouse_y);
    w.write_bool(bus.slave.device_mouse_updated);
    w.write_u8(bus.slave.mouse_btn);
    w.write_i32(bus.slave.center_x);
    w.write_i32(bus.slave.center_y);
    w.write_i32(bus.slave.input_mouse_x);
    w.write_i32(bus.slave.input_mouse_y);
    w.write_u8(bus.slave.input_mouse_btn);
    w.write_bool(bus.slave.irq_pending);
    w.write_bool(bus.slave.irq_deferred);
    w.write_bool(bus.slave.has_disc);
    w.write_u8(bus.slave.disc_state);
    w.write_u32(bus.slave.disc_timer);
    w.write_bool(bus.slave.xbus_interrupt_enable);
    w.write_u8(match bus.slave.video_mode {
        VideoMode::Pal => 0,
        VideoMode::Ntsc => 1,
    });
    w.write_u32(bus.slave.stream_counter);

    // 8. MCD212 state
    w.write_fixed_bytes(&bus.mcd212.plane_a[..]);
    w.write_fixed_bytes(&bus.mcd212.plane_b[..]);
    w.write_fixed_bytes(&bus.mcd212.csrr);
    for &v in &bus.mcd212.csrw { w.write_u16(v); }
    for &v in &bus.mcd212.dcr { w.write_u16(v); }
    for &v in &bus.mcd212.vsr { w.write_u16(v); }
    for &v in &bus.mcd212.ddr { w.write_u16(v); }
    for &v in &bus.mcd212.dcp { w.write_u16(v); }
    for &v in &bus.mcd212.dca { w.write_u32(v); }
    for &v in &bus.mcd212.clut { w.write_u32(v); }
    w.write_fixed_bytes(&bus.mcd212.clut_bank);
    w.write_u32(bus.mcd212.image_coding_method);
    w.write_u32(bus.mcd212.transparency_control);
    w.write_u32(bus.mcd212.plane_order);
    for &v in &bus.mcd212.transparent_color { w.write_u32(v); }
    for &v in &bus.mcd212.mask_color { w.write_u32(v); }
    for &v in &bus.mcd212.dyuv_abs_start { w.write_u32(v); }
    w.write_u32(bus.mcd212.cursor_position);
    w.write_u32(bus.mcd212.cursor_control);
    for &v in &bus.mcd212.cursor_pattern { w.write_u16(v); }
    for &v in &bus.mcd212.matte_control { w.write_u32(v); }
    w.write_u32(bus.mcd212.backdrop_color);
    for &v in &bus.mcd212.mosaic_hold { w.write_u32(v); }
    w.write_fixed_bytes(&bus.mcd212.weight_factor[0]);
    w.write_fixed_bytes(&bus.mcd212.weight_factor[1]);
    for p in 0..2 {
        for &b in &bus.mcd212.matte_flag[p] {
            w.write_bool(b);
        }
    }
    w.write_u16(bus.mcd212.blink_time);
    w.write_bool(bus.mcd212.blink_active);
    for &px in &bus.mcd212.framebuffer[..] {
        w.write_u32(px);
    }
    w.write_usize(bus.mcd212.scanline);
    w.write_u32(bus.mcd212.scanline_cycles);
    w.write_bool(bus.mcd212.vblank_irq);
    w.write_u8(match bus.mcd212.video_mode {
        VideoMode::Pal => 0,
        VideoMode::Ntsc => 1,
    });
    for &px in &bus.mcd212.external_video_line {
        w.write_u32(px);
    }

    // 9. CDIC state
    w.write_fixed_bytes(&bus.cdic.ram[..]);
    w.write_i16(bus.cdic.audio.left_state.s0);
    w.write_i16(bus.cdic.audio.left_state.s1);
    w.write_i16(bus.cdic.audio.right_state.s0);
    w.write_i16(bus.cdic.audio.right_state.s1);
    w.write_i16(bus.cdic.audio.map_left_state.s0);
    w.write_i16(bus.cdic.audio.map_left_state.s1);
    w.write_i16(bus.cdic.audio.map_right_state.s0);
    w.write_i16(bus.cdic.audio.map_right_state.s1);
    w.write_u64(bus.cdic.audio.total_sectors_decoded);
    w.write_bytes(&bus.cdic.audio.save_state());
    w.write_u16(bus.cdic.command);
    w.write_u32(bus.cdic.time);
    w.write_u16(bus.cdic.file);
    w.write_u32(bus.cdic.channel);
    w.write_u16(bus.cdic.audio_channel);
    w.write_u16(bus.cdic.audio_buffer);
    w.write_u16(bus.cdic.x_buffer);
    w.write_u16(bus.cdic.dma_control);
    w.write_u16(bus.cdic.z_buffer);
    w.write_u16(bus.cdic.interrupt_vector);
    w.write_u16(bus.cdic.data_buffer);
    w.write_u8(bus.cdic.disc_command);
    w.write_u8(bus.cdic.disc_mode);
    w.write_u8(bus.cdic.disc_spinup_counter);
    w.write_u32(bus.cdic.curr_lba);
    w.write_bool(bus.cdic.decoding_audio_map);
    w.write_u16(bus.cdic.decode_addr);
    w.write_u8(bus.cdic.audio_sector_counter);
    w.write_u8(bus.cdic.audio_map_counter);
    w.write_u8(bus.cdic.audio_format_sectors);
    w.write_u32(bus.cdic.audio_cycle_accumulator);
    w.write_u32(bus.cdic.cycle_accumulator);
    w.write_bool(bus.cdic.irq_pending);
    w.write_bool(bus.cdic.dma_triggered);
    w.write_bool(bus.cdic.cd_byteswap);

    // 10. DVC state
    w.write_fixed_bytes(&bus.dvc.ram1[..]);
    w.write_fixed_bytes(&bus.dvc.ram2[..]);
    w.write_bool(bus.dvc.ram2_enabled);
    w.write_u32(bus.dvc.reg_write_count);
    w.write_u32(bus.dvc.reg_read_count);
    w.write_u32(bus.dvc.rom_read_count);
    w.write_fixed_bytes(&bus.dvc.fma_regs[..]);
    w.write_fixed_bytes(&bus.dvc.fmv_regs[..]);
    w.write_u16(bus.dvc.fma_cmd);
    w.write_u16(bus.dvc.fma_isr);
    w.write_u16(bus.dvc.fma_ier);
    w.write_u16(bus.dvc.fma_ivec);
    w.write_u16(bus.dvc.fmv_isr);
    w.write_u16(bus.dvc.fmv_ier);
    w.write_u16(bus.dvc.fmv_ivec);
    w.write_bool(bus.dvc.fmv_playback_active);
    w.write_bool(bus.dvc.fmv_decoder_active);
    w.write_u64(bus.dvc.total_cycles);
    w.write_u32(bus.dvc.dclk);
    w.write_u64(bus.dvc.dclk_accum);
    w.write_u64(bus.dvc.timer_accum);
    w.write_u16(bus.dvc.timer_counter);
    w.write_bool(bus.dvc.video_playing);
    w.write_bool(bus.dvc.video_armed);
    w.write_bool(bus.dvc.decoder_enabled);
    w.write_bool(bus.dvc.audio_armed);
    w.write_bool(bus.dvc.audio_enabled);
    w.write_bool(bus.dvc.vcd_pixel_clock_13_5);
    w.write_u32(bus.dvc.display_timecode);
    w.write_u32(bus.dvc.decoder_timecode);
    w.write_u32(bus.dvc.display_frame_counter);
    w.write_u32(bus.dvc.decoder_frame_counter);
    w.write_f64(bus.dvc.fps);
    w.write_f64(bus.dvc.frame_accum);
    w.write_f64(bus.dvc.vsync_hz);
    w.write_bytes(&bus.dvc.dvc_stream_fifo);
    w.write_bytes(&bus.dvc.fma_stream_fifo);
    w.write_bool(bus.dvc.transfer_for_fma);
    w.write_bool(bus.dvc.fma_dma_seen);
    w.write_bool(bus.dvc.dma_requested);
    for &px in &bus.dvc.framebuffer[..] {
        w.write_u32(px);
    }

    // Demux & FMA Demux
    for dm in [&bus.dvc.demux, &bus.dvc.fma_demux] {
        w.write_bytes(&dm.pending);
        w.write_bytes(&dm.video);
        let audio_vec: Vec<u8> = dm.audio.iter().copied().collect();
        w.write_bytes(&audio_vec);
        w.write_opt_u64(dm.last_scr);
        w.write_opt_u64(dm.first_video_pts);
        w.write_opt_u64(dm.last_video_pts);
        w.write_opt_u64(dm.last_video_dts);
        w.write_opt_u64(dm.first_audio_pts);
        w.write_opt_u64(dm.last_audio_pts);
        w.write_opt_u8(dm.selected_audio_stream);
        w.write_opt_u8(dm.selected_video_stream);
        w.write_u64(dm.program_ends);
        w.write_opt_u64(dm.scr_base);
        w.write_u64(dm.mux_rate);
        w.write_u64(dm.pack_bytes);
        w.write_u64(dm.tick_bytes);
        w.write_u32(dm.due.len() as u32);
        for &d in &dm.due {
            w.write_u64(d);
        }
        w.write_opt_u64(dm.last_due);
        w.write_u64(dm.interval);
    }

    w.write_u32(bus.dvc.audio_sample_rate);
    w.write_usize(bus.dvc.video_width);
    w.write_usize(bus.dvc.video_height);
    w.write_usize(bus.dvc.frames_decoded_count);
    w.write_usize(bus.dvc.frames_presented_count);
    w.write_usize(bus.dvc.vsync_counter);
    w.write_u32(bus.dvc.underflow_count);
    w.write_bool(bus.dvc.fma_started);
    w.write_u64(bus.dvc.fma_tick_accum);
    w.write_u16(bus.dvc.fma_pending);
    w.write_u32(bus.dvc.fma_last_header);
    w.write_bool(bus.dvc.has_valid_frame);

    // Video Decoder
    match &bus.dvc.video_decoder {
        Some(vd) => {
            w.write_bool(true);
            w.write_bytes(&vd.buffer.bytes);
            w.write_usize(vd.buffer.bit_index);
            w.write_f64(vd.framerate);
            w.write_f64(vd.pixel_aspect_ratio);
            w.write_f64(vd.time);
            w.write_f64(vd.last_frame_time);
            w.write_i64(vd.frames_decoded);
            w.write_usize(vd.width);
            w.write_usize(vd.height);
            w.write_usize(vd.mb_width);
            w.write_usize(vd.mb_height);
            w.write_usize(vd.mb_size);
            w.write_usize(vd.luma_width);
            w.write_usize(vd.luma_height);
            w.write_usize(vd.chroma_width);
            w.write_usize(vd.chroma_height);
            w.write_i32(vd.start_code);
            w.write_i32(vd.picture_type);
            for m in [&vd.motion_forward, &vd.motion_backward] {
                w.write_bool(m.full_px);
                w.write_bool(m.is_set);
                w.write_i32(m.r_size);
                w.write_i32(m.h);
                w.write_i32(m.v);
            }
            w.write_bool(vd.has_sequence_header);
            w.write_i32(vd.quantizer_scale);
            w.write_bool(vd.slice_begin);
            w.write_i32(vd.macroblock_address);
            w.write_usize(vd.mb_row);
            w.write_usize(vd.mb_col);
            w.write_i32(vd.macroblock_type);
            w.write_bool(vd.macroblock_intra);
            for &dc in &vd.dc_predictor { w.write_i32(dc); }
            w.write_usize(vd.frames.len());
            for f in &vd.frames {
                w.write_usize(f.width);
                w.write_usize(f.height);
                for pl in [&f.y, &f.cr, &f.cb] {
                    w.write_usize(pl.width);
                    w.write_usize(pl.height);
                    w.write_bytes(&pl.data);
                }
            }
            w.write_usize(vd.cur);
            w.write_usize(vd.fwd);
            w.write_usize(vd.bwd);
            for &bd in &vd.block_data { w.write_i32(bd); }
            w.write_fixed_bytes(&vd.intra_quant_matrix);
            w.write_fixed_bytes(&vd.non_intra_quant_matrix);
            w.write_bool(vd.has_reference_frame);
            w.write_bool(vd.assume_no_b_frames);
            w.write_bool(vd.sequence_end_seen);
            w.write_bool(vd.sequence_header_seen);
            w.write_bool(vd.gop_header_seen);
            w.write_opt_u32(vd.last_gop_timecode);
            w.write_u16(vd.prpa);
        }
        None => w.write_bool(false),
    }

    let p = path.as_ref();
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    let tmp = std::path::PathBuf::from(format!("{}.tmp", p.display()));
    fs::write(&tmp, &w.buf)?;
    fs::rename(&tmp, p)?;
    Ok(())
}

pub fn load_state_from_file<P: AsRef<Path>>(
    path: P,
    cpu: &mut CpuCore,
    bus: &mut CdiBus,
    disc_launched: &mut bool,
    shell_ready_count: &mut u32,
    pad_x: &mut i32,
    pad_y: &mut i32,
) -> io::Result<()> {
    let data = fs::read(path)?;
    let mut r = StateReader::new(&data);

    let mut magic = [0u8; 8];
    r.read_fixed_bytes(&mut magic)?;
    if &magic != SAVESTATE_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid save state file header (not an OpenCDi save state)",
        ));
    }

    // 1. Emulation frontend state
    *disc_launched = r.read_bool()?;
    *shell_ready_count = r.read_u32()?;
    *pad_x = r.read_i32()?;
    *pad_y = r.read_i32()?;

    // 2. CPU Core state
    cpu.set_cpu_type(CpuType::SCC68070);
    for reg in &mut cpu.dar { *reg = r.read_u32()?; }
    for reg in &mut cpu.dar_save { *reg = r.read_u32()?; }
    cpu.sr_save = r.read_u16()?;
    cpu.ppc = r.read_u32()?;
    cpu.pc = r.read_u32()?;
    for s in &mut cpu.sp { *s = r.read_u32()?; }
    cpu.vbr = r.read_u32()?;
    cpu.sfc = r.read_u32()?;
    cpu.dfc = r.read_u32()?;
    cpu.cacr = r.read_u32()?;
    cpu.caar = r.read_u32()?;
    cpu.ir = r.read_u32()?;
    cpu.t1_flag = r.read_u32()?;
    cpu.t0_flag = r.read_u32()?;
    cpu.s_flag = r.read_u32()?;
    cpu.m_flag = r.read_u32()?;
    cpu.x_flag = r.read_u32()?;
    cpu.n_flag = r.read_u32()?;
    cpu.not_z_flag = r.read_u32()?;
    cpu.v_flag = r.read_u32()?;
    cpu.c_flag = r.read_u32()?;
    cpu.int_mask = r.read_u32()?;
    cpu.int_level = r.read_u32()?;
    cpu.stopped = r.read_u32()?;
    cpu.change_of_flow = r.read_bool()?;
    cpu.loop_mode = r.read_bool()?;
    cpu.loop_body_word = r.read_u16()?;
    cpu.loop_dbcc_word = r.read_u16()?;
    cpu.prefetch_queue[0] = r.read_u16()?;
    cpu.prefetch_queue[1] = r.read_u16()?;
    cpu.prefetch_count = r.read_u8()?;
    cpu.consume_without_prefetch = r.read_bool()?;
    cpu.pending_sync_clocks = r.read_u32()?;
    cpu.instr_mode = r.read_u32()?;
    cpu.run_mode = r.read_u32()?;
    cpu.exception_processing = r.read_bool()?;
    cpu.virq_state = r.read_u32()?;
    cpu.nmi_pending = r.read_u32()?;
    cpu.cycles_remaining = r.read_i32()?;
    cpu.initial_cycles = r.read_i32()?;

    // 3. CdiBus state
    r.read_fixed_bytes(&mut bus.sys_ram[..])?;
    bus.shared_in4_owner = match r.read_u8()? {
        1 => SharedIn4Owner::Cdic,
        2 => SharedIn4Owner::Vmpeg,
        _ => SharedIn4Owner::Idle,
    };
    bus.scc_uart_cmd = r.read_u8()?;
    bus.scc_uart_tx_irq = r.read_bool()?;
    bus.current_pc = r.read_u32()?;
    bus.wait_cycles = r.read_u32()?;
    bus.current_line_cycle = r.read_u32()?;
    bus.emulate_wait_states = r.read_bool()?;
    bus.current_frame = r.read_u64()?;

    // 4. SccTimer state
    bus.scc_timer.tsr = r.read_u8()?;
    bus.scc_timer.tcr = r.read_u8()?;
    bus.scc_timer.reload = r.read_u16()?;
    bus.scc_timer.timer0 = r.read_u16()?;
    bus.scc_timer.timer1 = r.read_u16()?;
    bus.scc_timer.timer2 = r.read_u16()?;
    bus.scc_timer.picr1 = r.read_u8()?;
    bus.scc_timer.picr2 = r.read_u8()?;
    bus.scc_timer.lir = r.read_u8()?;
    bus.scc_timer.int1_line = r.read_bool()?;
    bus.scc_timer.timer_irq = r.read_bool()?;
    bus.scc_timer.cycle_accum = r.read_u32()?;

    // 5. SccDma state
    for ch in &mut bus.dma.channels {
        ch.channel_status = r.read_u8()?;
        ch.channel_error = r.read_u8()?;
        ch.device_control = r.read_u8()?;
        ch.operation_control = r.read_u8()?;
        ch.sequence_control = r.read_u8()?;
        ch.channel_control = r.read_u8()?;
        ch.transfer_counter = r.read_u16()?;
        ch.memory_address_counter = r.read_u32()?;
        ch.device_address_counter = r.read_u32()?;
    }

    // 6. NVRAM state
    r.read_fixed_bytes(&mut bus.nvram.ram[..])?;
    bus.nvram.dirty = true;
    bus.nvram.flush_if_dirty();

    // 7. SlaveHle state
    for ch in &mut bus.slave.channels {
        r.read_fixed_bytes(&mut ch.out_buf)?;
        ch.out_count = r.read_usize()?;
        ch.out_index = r.read_usize()?;
        ch.out_cmd = r.read_u8()?;
        r.read_fixed_bytes(&mut ch.in_buf)?;
        ch.in_count = r.read_usize()?;
        ch.in_index = r.read_usize()?;
        r.read_fixed_bytes(&mut ch.pending_buf)?;
        ch.pending_count = r.read_usize()?;
        ch.pending_cmd = r.read_u8()?;
    }
    bus.slave.device_mouse_x = r.read_i32()?;
    bus.slave.device_mouse_y = r.read_i32()?;
    bus.slave.device_mouse_updated = r.read_bool()?;
    bus.slave.mouse_btn = r.read_u8()?;
    bus.slave.center_x = r.read_i32()?;
    bus.slave.center_y = r.read_i32()?;
    bus.slave.input_mouse_x = r.read_i32()?;
    bus.slave.input_mouse_y = r.read_i32()?;
    bus.slave.input_mouse_btn = r.read_u8()?;
    bus.slave.irq_pending = r.read_bool()?;
    bus.slave.irq_deferred = r.read_bool()?;
    bus.slave.has_disc = r.read_bool()?;
    bus.slave.disc_state = r.read_u8()?;
    bus.slave.disc_timer = r.read_u32()?;
    bus.slave.xbus_interrupt_enable = r.read_bool()?;
    bus.slave.video_mode = if r.read_u8()? == 1 { VideoMode::Ntsc } else { VideoMode::Pal };
    bus.slave.stream_counter = r.read_u32()?;

    // 8. MCD212 state
    r.read_fixed_bytes(&mut bus.mcd212.plane_a[..])?;
    r.read_fixed_bytes(&mut bus.mcd212.plane_b[..])?;
    r.read_fixed_bytes(&mut bus.mcd212.csrr)?;
    for v in &mut bus.mcd212.csrw { *v = r.read_u16()?; }
    for v in &mut bus.mcd212.dcr { *v = r.read_u16()?; }
    for v in &mut bus.mcd212.vsr { *v = r.read_u16()?; }
    for v in &mut bus.mcd212.ddr { *v = r.read_u16()?; }
    for v in &mut bus.mcd212.dcp { *v = r.read_u16()?; }
    for v in &mut bus.mcd212.dca { *v = r.read_u32()?; }
    for v in &mut bus.mcd212.clut { *v = r.read_u32()?; }
    r.read_fixed_bytes(&mut bus.mcd212.clut_bank)?;
    bus.mcd212.image_coding_method = r.read_u32()?;
    bus.mcd212.transparency_control = r.read_u32()?;
    bus.mcd212.plane_order = r.read_u32()?;
    for v in &mut bus.mcd212.transparent_color { *v = r.read_u32()?; }
    for v in &mut bus.mcd212.mask_color { *v = r.read_u32()?; }
    for v in &mut bus.mcd212.dyuv_abs_start { *v = r.read_u32()?; }
    bus.mcd212.cursor_position = r.read_u32()?;
    bus.mcd212.cursor_control = r.read_u32()?;
    for v in &mut bus.mcd212.cursor_pattern { *v = r.read_u16()?; }
    for v in &mut bus.mcd212.matte_control { *v = r.read_u32()?; }
    bus.mcd212.backdrop_color = r.read_u32()?;
    for v in &mut bus.mcd212.mosaic_hold { *v = r.read_u32()?; }
    r.read_fixed_bytes(&mut bus.mcd212.weight_factor[0])?;
    r.read_fixed_bytes(&mut bus.mcd212.weight_factor[1])?;
    for p in 0..2 {
        for b in &mut bus.mcd212.matte_flag[p] {
            *b = r.read_bool()?;
        }
    }
    bus.mcd212.blink_time = r.read_u16()?;
    bus.mcd212.blink_active = r.read_bool()?;
    for px in &mut bus.mcd212.framebuffer[..] {
        *px = r.read_u32()?;
    }
    bus.mcd212.scanline = r.read_usize()?;
    bus.mcd212.scanline_cycles = r.read_u32()?;
    bus.mcd212.vblank_irq = r.read_bool()?;
    bus.mcd212.video_mode = if r.read_u8()? == 1 { VideoMode::Ntsc } else { VideoMode::Pal };
    for px in &mut bus.mcd212.external_video_line {
        *px = r.read_u32()?;
    }

    // 9. CDIC state
    bus.cdic.audio.reset();
    r.read_fixed_bytes(&mut bus.cdic.ram[..])?;
    bus.cdic.audio.left_state.s0 = r.read_i16()?;
    bus.cdic.audio.left_state.s1 = r.read_i16()?;
    bus.cdic.audio.right_state.s0 = r.read_i16()?;
    bus.cdic.audio.right_state.s1 = r.read_i16()?;
    bus.cdic.audio.map_left_state.s0 = r.read_i16()?;
    bus.cdic.audio.map_left_state.s1 = r.read_i16()?;
    bus.cdic.audio.map_right_state.s0 = r.read_i16()?;
    bus.cdic.audio.map_right_state.s1 = r.read_i16()?;
    bus.cdic.audio.total_sectors_decoded = r.read_u64()?;
    let mixer = r.read_bytes()?;
    bus.cdic.audio.load_state(&mixer).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad audio mixer state"))?;
    bus.cdic.command = r.read_u16()?;
    bus.cdic.time = r.read_u32()?;
    bus.cdic.file = r.read_u16()?;
    bus.cdic.channel = r.read_u32()?;
    bus.cdic.audio_channel = r.read_u16()?;
    bus.cdic.audio_buffer = r.read_u16()?;
    bus.cdic.x_buffer = r.read_u16()?;
    bus.cdic.dma_control = r.read_u16()?;
    bus.cdic.z_buffer = r.read_u16()?;
    bus.cdic.interrupt_vector = r.read_u16()?;
    bus.cdic.data_buffer = r.read_u16()?;
    bus.cdic.disc_command = r.read_u8()?;
    bus.cdic.disc_mode = r.read_u8()?;
    bus.cdic.disc_spinup_counter = r.read_u8()?;
    bus.cdic.curr_lba = r.read_u32()?;
    bus.cdic.decoding_audio_map = r.read_bool()?;
    bus.cdic.decode_addr = r.read_u16()?;
    bus.cdic.audio_sector_counter = r.read_u8()?;
    bus.cdic.audio_map_counter = r.read_u8()?;
    bus.cdic.audio_format_sectors = r.read_u8()?;
    bus.cdic.audio_cycle_accumulator = r.read_u32()?;
    bus.cdic.cycle_accumulator = r.read_u32()?;
    bus.cdic.irq_pending = r.read_bool()?;
    bus.cdic.dma_triggered = r.read_bool()?;
    bus.cdic.cd_byteswap = r.read_bool()?;

    // 10. DVC state
    r.read_fixed_bytes(&mut bus.dvc.ram1[..])?;
    r.read_fixed_bytes(&mut bus.dvc.ram2[..])?;
    bus.dvc.ram2_enabled = r.read_bool()?;
    bus.dvc.reg_write_count = r.read_u32()?;
    bus.dvc.reg_read_count = r.read_u32()?;
    bus.dvc.rom_read_count = r.read_u32()?;
    r.read_fixed_bytes(&mut bus.dvc.fma_regs[..])?;
    r.read_fixed_bytes(&mut bus.dvc.fmv_regs[..])?;
    bus.dvc.fma_cmd = r.read_u16()?;
    bus.dvc.fma_isr = r.read_u16()?;
    bus.dvc.fma_ier = r.read_u16()?;
    bus.dvc.fma_ivec = r.read_u16()?;
    bus.dvc.fmv_isr = r.read_u16()?;
    bus.dvc.fmv_ier = r.read_u16()?;
    bus.dvc.fmv_ivec = r.read_u16()?;
    bus.dvc.fmv_playback_active = r.read_bool()?;
    bus.dvc.fmv_decoder_active = r.read_bool()?;
    bus.dvc.total_cycles = r.read_u64()?;
    bus.dvc.dclk = r.read_u32()?;
    bus.dvc.dclk_accum = r.read_u64()?;
    bus.dvc.timer_accum = r.read_u64()?;
    bus.dvc.timer_counter = r.read_u16()?;
    bus.dvc.video_playing = r.read_bool()?;
    bus.dvc.video_armed = r.read_bool()?;
    bus.dvc.decoder_enabled = r.read_bool()?;
    bus.dvc.audio_armed = r.read_bool()?;
    bus.dvc.audio_enabled = r.read_bool()?;
    bus.dvc.vcd_pixel_clock_13_5 = r.read_bool()?;
    bus.dvc.display_timecode = r.read_u32()?;
    bus.dvc.decoder_timecode = r.read_u32()?;
    bus.dvc.display_frame_counter = r.read_u32()?;
    bus.dvc.decoder_frame_counter = r.read_u32()?;
    bus.dvc.fps = r.read_f64()?;
    bus.dvc.frame_accum = r.read_f64()?;
    bus.dvc.vsync_hz = r.read_f64()?;
    bus.dvc.dvc_stream_fifo = r.read_bytes()?;
    bus.dvc.fma_stream_fifo = r.read_bytes()?;
    bus.dvc.transfer_for_fma = r.read_bool()?;
    bus.dvc.fma_dma_seen = r.read_bool()?;
    bus.dvc.dma_requested = r.read_bool()?;
    for px in &mut bus.dvc.framebuffer[..] {
        *px = r.read_u32()?;
    }

    for dm in [&mut bus.dvc.demux, &mut bus.dvc.fma_demux] {
        dm.pending = r.read_bytes()?;
        dm.video = r.read_bytes()?;
        let audio_vec = r.read_bytes()?;
        dm.audio = VecDeque::from(audio_vec);
        dm.last_scr = r.read_opt_u64()?;
        dm.first_video_pts = r.read_opt_u64()?;
        dm.last_video_pts = r.read_opt_u64()?;
        dm.last_video_dts = r.read_opt_u64()?;
        dm.first_audio_pts = r.read_opt_u64()?;
        dm.last_audio_pts = r.read_opt_u64()?;
        dm.selected_audio_stream = r.read_opt_u8()?;
        dm.selected_video_stream = r.read_opt_u8()?;
        dm.program_ends = r.read_u64()?;
        dm.scr_base = r.read_opt_u64()?;
        dm.mux_rate = r.read_u64()?;
        dm.pack_bytes = r.read_u64()?;
        dm.tick_bytes = r.read_u64()?;
        let n = r.read_u32()?;
        dm.due.clear();
        if n as usize > crate::dvc::DUE_RING {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "DVC audio schedule too long in save state"));
        }
        for _ in 0..n {
            dm.due.push_back(r.read_u64()?);
        }
        dm.last_due = r.read_opt_u64()?;
        dm.interval = r.read_u64()?;
        dm.events.clear();
    }

    bus.dvc.mp2.reset();
    bus.dvc.audio_out.clear();
    bus.dvc.audio_sample_rate = r.read_u32()?;
    bus.dvc.video_width = r.read_usize()?;
    bus.dvc.video_height = r.read_usize()?;
    bus.dvc.frames_decoded_count = r.read_usize()?;
    bus.dvc.frames_presented_count = r.read_usize()?;
    bus.dvc.vsync_counter = r.read_usize()?;
    bus.dvc.underflow_count = r.read_u32()?;
    bus.dvc.fma_started = r.read_bool()?;
    bus.dvc.fma_tick_accum = r.read_u64()?;
    bus.dvc.fma_pending = r.read_u16()?;
    bus.dvc.fma_last_header = r.read_u32()?;
    bus.dvc.has_valid_frame = r.read_bool()?;

    if r.read_bool()? {
        let bytes = r.read_bytes()?;
        let bit_index = r.read_usize()?;
        let framerate = r.read_f64()?;
        let pixel_aspect_ratio = r.read_f64()?;
        let time = r.read_f64()?;
        let last_frame_time = r.read_f64()?;
        let frames_decoded = r.read_i64()?;
        let width = r.read_usize()?;
        let height = r.read_usize()?;
        let mb_width = r.read_usize()?;
        let mb_height = r.read_usize()?;
        let mb_size = r.read_usize()?;
        let luma_width = r.read_usize()?;
        let luma_height = r.read_usize()?;
        let chroma_width = r.read_usize()?;
        let chroma_height = r.read_usize()?;
        let start_code = r.read_i32()?;
        let picture_type = r.read_i32()?;
        let mut motions = [Motion::default(); 2];
        for m in &mut motions {
            m.full_px = r.read_bool()?;
            m.is_set = r.read_bool()?;
            m.r_size = r.read_i32()?;
            m.h = r.read_i32()?;
            m.v = r.read_i32()?;
        }
        let has_sequence_header = r.read_bool()?;
        let quantizer_scale = r.read_i32()?;
        let slice_begin = r.read_bool()?;
        let macroblock_address = r.read_i32()?;
        let mb_row = r.read_usize()?;
        let mb_col = r.read_usize()?;
        let macroblock_type = r.read_i32()?;
        let macroblock_intra = r.read_bool()?;
        let mut dc_predictor = [0i32; 3];
        for dc in &mut dc_predictor { *dc = r.read_i32()?; }
        let frames_len = r.read_usize()?;
        let mut frames = Vec::with_capacity(frames_len);
        for _ in 0..frames_len {
            let fw = r.read_usize()?;
            let fh = r.read_usize()?;
            let y_w = r.read_usize()?;
            let y_h = r.read_usize()?;
            let y_d = r.read_bytes()?;
            let cr_w = r.read_usize()?;
            let cr_h = r.read_usize()?;
            let cr_d = r.read_bytes()?;
            let cb_w = r.read_usize()?;
            let cb_h = r.read_usize()?;
            let cb_d = r.read_bytes()?;
            frames.push(Frame {
                width: fw,
                height: fh,
                y: Plane { width: y_w, height: y_h, data: y_d },
                cr: Plane { width: cr_w, height: cr_h, data: cr_d },
                cb: Plane { width: cb_w, height: cb_h, data: cb_d },
            });
        }
        let cur = r.read_usize()?;
        let fwd = r.read_usize()?;
        let bwd = r.read_usize()?;
        let mut block_data = [0i32; 64];
        for bd in &mut block_data { *bd = r.read_i32()?; }
        let mut intra_quant_matrix = [0u8; 64];
        r.read_fixed_bytes(&mut intra_quant_matrix)?;
        let mut non_intra_quant_matrix = [0u8; 64];
        r.read_fixed_bytes(&mut non_intra_quant_matrix)?;
        let has_reference_frame = r.read_bool()?;
        let assume_no_b_frames = r.read_bool()?;
        let sequence_end_seen = r.read_bool()?;
        let sequence_header_seen = r.read_bool()?;
        let gop_header_seen = r.read_bool()?;
        let last_gop_timecode = r.read_opt_u32()?;
        let prpa = r.read_u16()?;

        bus.dvc.video_decoder = Some(Video {
            buffer: Buffer { bytes, bit_index },
            framerate,
            pixel_aspect_ratio,
            time,
            last_frame_time,
            frames_decoded,
            width,
            height,
            mb_width,
            mb_height,
            mb_size,
            luma_width,
            luma_height,
            chroma_width,
            chroma_height,
            start_code,
            picture_type,
            motion_forward: motions[0],
            motion_backward: motions[1],
            has_sequence_header,
            quantizer_scale,
            slice_begin,
            macroblock_address,
            mb_row,
            mb_col,
            macroblock_type,
            macroblock_intra,
            dc_predictor,
            frames,
            cur,
            fwd,
            bwd,
            block_data,
            intra_quant_matrix,
            non_intra_quant_matrix,
            has_reference_frame,
            assume_no_b_frames,
            sequence_end_seen,
            sequence_header_seen,
            gop_header_seen,
            last_gop_timecode,
            prpa,
        });
    } else {
        bus.dvc.video_decoder = None;
    }

    Ok(())
}

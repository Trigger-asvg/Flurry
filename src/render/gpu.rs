//! Direct3D 11 + DirectComposition: одно устройство на всё, по swapchain-у на монитор.
//! Частицы рисуются инстансингом (один DrawInstanced на монитор), CPU только заполняет буфер ~48 байт на частицу.

use windows::core::{s, Interface, Result, PCSTR};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Inst {
    /// x, y (центр, px), половина ширины, половина высоты
    pub a: [f32; 4],
    /// поворот (рад), вид, p1, p2
    pub b: [f32; 4],
    /// цвет RGBA (не premultiplied)
    pub c: [f32; 4],
}

#[repr(C)]
struct Consts {
    scr: [f32; 2],
    tm: f32,
    op: f32,
}

pub struct Gpu {
    pub device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    factory: IDXGIFactory2,
    pub dcomp: IDCompositionDevice,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    cb: ID3D11Buffer,
    vb: ID3D11Buffer,
    vb_cap: usize,
    pub warp: bool,
}

pub struct Target {
    sc: IDXGISwapChain1,
    rtv: ID3D11RenderTargetView,
    _target: IDCompositionTarget,
    root: IDCompositionVisual,
    visual: IDCompositionVisual,
    pub w: u32,
    pub h: u32,
    empty: bool,
    /// Слой «фона» низкого разрешения (туман, вспышки): DirectComposition сам растягивает его на весь экран.
    pub bg: Option<BgLayer>,
}

pub struct BgLayer {
    sc: IDXGISwapChain1,
    rtv: ID3D11RenderTargetView,
    _visual: IDCompositionVisual,
    pub w: u32,
    pub h: u32,
    empty: bool,
}

/// Во сколько раз слой фона меньше экрана.
pub const BG_DIV: u32 = 8;

fn chain_desc(w: u32, h: u32) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
        Width: w.max(1),
        Height: h.max(1),
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    }
}

const SHADER: &str = include_str!("shader.hlsl");

/// Скомпилированные шейдеры (vs, ps): компилируем один раз за жизнь процесса, а не при каждом
/// пересоздании устройства (после выключения эффекта, потери устройства и т.п.).
static SHADERS: std::sync::Mutex<Option<(Vec<u8>, Vec<u8>)>> = std::sync::Mutex::new(None);

fn shaders() -> Result<(Vec<u8>, Vec<u8>)> {
    let mut g = SHADERS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(b) = g.as_ref() {
        return Ok(b.clone());
    }
    let b = (compile(s!("vs"), s!("vs_4_0"))?, compile(s!("ps"), s!("ps_4_0"))?);
    *g = Some(b.clone());
    Ok(b)
}

fn compile(entry: PCSTR, target: PCSTR) -> Result<Vec<u8>> {
    unsafe {
        let mut blob: Option<ID3DBlob> = None;
        let mut err: Option<ID3DBlob> = None;
        let r = D3DCompile(
            SHADER.as_ptr() as _,
            SHADER.len(),
            s!("hz"),
            None,
            None,
            entry,
            target,
            1 << 15, // D3DCOMPILE_OPTIMIZATION_LEVEL3
            0,
            &mut blob,
            Some(&mut err),
        );
        if let Err(e) = r {
            if let Some(err) = err {
                let msg = std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize());
                crate::settings::log(&format!("shader: {}", String::from_utf8_lossy(msg)));
            }
            return Err(e);
        }
        let b = blob.unwrap();
        Ok(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec())
    }
}

impl Gpu {
    pub fn new() -> Result<Gpu> {
        unsafe {
            let levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
            let mut device = None;
            let mut ctx = None;
            let mut warp = false;
            let hw = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            );
            if hw.is_err() {
                warp = true;
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut ctx),
                )?;
            }
            let device: ID3D11Device = device.unwrap();
            let ctx: ID3D11DeviceContext = ctx.unwrap();
            let dxgi: IDXGIDevice = device.cast()?;
            let adapter = dxgi.GetAdapter()?;
            let factory: IDXGIFactory2 = adapter.GetParent()?;
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;

            let (vsb, psb) = shaders()?;
            let mut vs = None;
            device.CreateVertexShader(&vsb, None, Some(&mut vs))?;
            let mut ps = None;
            device.CreatePixelShader(&psb, None, Some(&mut ps))?;
            let el = |i: u32| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("TEXCOORD"),
                SemanticIndex: i,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: i * 16,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            };
            let mut layout = None;
            device.CreateInputLayout(&[el(0), el(1), el(2)], &vsb, Some(&mut layout))?;

            let mut bd = D3D11_BLEND_DESC::default();
            bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend = None;
            device.CreateBlendState(&bd, Some(&mut blend))?;
            let rd = D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                DepthClipEnable: true.into(),
                ..Default::default()
            };
            let mut raster = None;
            device.CreateRasterizerState(&rd, Some(&mut raster))?;
            let cbd = D3D11_BUFFER_DESC {
                ByteWidth: 16,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            };
            let mut cb = None;
            device.CreateBuffer(&cbd, None, Some(&mut cb))?;
            let vb_cap = 4096;
            let vb = Self::make_vb(&device, vb_cap)?;
            Ok(Gpu {
                device,
                ctx,
                factory,
                dcomp,
                vs: vs.unwrap(),
                ps: ps.unwrap(),
                layout: layout.unwrap(),
                blend: blend.unwrap(),
                raster: raster.unwrap(),
                cb: cb.unwrap(),
                vb,
                vb_cap,
                warp,
            })
        }
    }

    fn make_vb(device: &ID3D11Device, n: usize) -> Result<ID3D11Buffer> {
        let d = D3D11_BUFFER_DESC {
            ByteWidth: (n * std::mem::size_of::<Inst>()) as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut b = None;
        unsafe { device.CreateBuffer(&d, None, Some(&mut b))? };
        Ok(b.unwrap())
    }

    pub fn target(&self, hwnd: HWND, w: u32, h: u32) -> Result<Target> {
        unsafe {
            let sc = self.factory.CreateSwapChainForComposition(&self.device, &chain_desc(w, h), None)?;
            let back: ID3D11Texture2D = sc.GetBuffer(0)?;
            let mut rtv = None;
            self.device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
            let target = self.dcomp.CreateTargetForHwnd(hwnd, true)?;
            let root = self.dcomp.CreateVisual()?;
            let visual = self.dcomp.CreateVisual()?;
            visual.SetContent(&sc)?;
            root.AddVisual(&visual, false, None)?;
            target.SetRoot(&root)?;
            self.dcomp.Commit()?;
            Ok(Target { sc, rtv: rtv.unwrap(), _target: target, root, visual, w, h, empty: false, bg: None })
        }
    }

    /// Рисует слой фона (координаты инстансов — уже в его уменьшенном масштабе). Создаётся при первом использовании.
    pub fn render_bg(&mut self, t: &mut Target, inst: &[Inst], time: f32, opacity: f32) -> Result<()> {
        if t.bg.is_none() {
            if inst.is_empty() {
                return Ok(());
            }
            unsafe {
                let (w, h) = ((t.w / BG_DIV).max(1), (t.h / BG_DIV).max(1));
                let sc = self.factory.CreateSwapChainForComposition(&self.device, &chain_desc(w, h), None)?;
                let back: ID3D11Texture2D = sc.GetBuffer(0)?;
                let mut rtv = None;
                self.device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
                let v = self.dcomp.CreateVisual()?;
                v.SetContent(&sc)?;
                let m = windows::Foundation::Numerics::Matrix3x2 { M11: t.w as f32 / w as f32, M12: 0.0, M21: 0.0, M22: t.h as f32 / h as f32, M31: 0.0, M32: 0.0 };
                v.SetTransform2(&m)?;
                v.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR)?;
                t.root.AddVisual(&v, false, &t.visual)?; // под частицами
                self.dcomp.Commit()?;
                t.bg = Some(BgLayer { sc, rtv: rtv.unwrap(), _visual: v, w, h, empty: false });
            }
        }
        let bg = t.bg.as_mut().unwrap();
        if inst.is_empty() && bg.empty {
            return Ok(());
        }
        let (sc, rtv, w, h) = (bg.sc.clone(), bg.rtv.clone(), bg.w, bg.h);
        self.draw(&sc, &rtv, w, h, inst, time, opacity)?;
        t.bg.as_mut().unwrap().empty = inst.is_empty();
        Ok(())
    }

    /// Рисует кадр в swapchain монитора и показывает его. Пустые кадры подряд не отправляются.
    pub fn render(&mut self, t: &mut Target, inst: &[Inst], time: f32, opacity: f32) -> Result<()> {
        if inst.is_empty() && t.empty {
            return Ok(());
        }
        let (sc, rtv, w, h) = (t.sc.clone(), t.rtv.clone(), t.w, t.h);
        self.draw(&sc, &rtv, w, h, inst, time, opacity)?;
        t.empty = inst.is_empty();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&mut self, sc: &IDXGISwapChain1, rtv: &ID3D11RenderTargetView, w: u32, h: u32, inst: &[Inst], time: f32, opacity: f32) -> Result<()> {
        unsafe {
            if inst.len() > self.vb_cap {
                self.vb_cap = inst.len().next_power_of_two();
                self.vb = Self::make_vb(&self.device, self.vb_cap)?;
            }
            let c = &self.ctx;
            if !inst.is_empty() {
                let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                c.Map(&self.vb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m))?;
                std::ptr::copy_nonoverlapping(inst.as_ptr(), m.pData as *mut Inst, inst.len());
                c.Unmap(&self.vb, 0);
            }
            let k = Consts { scr: [w as f32, h as f32], tm: time, op: opacity };
            c.UpdateSubresource(&self.cb, 0, None, &k as *const _ as _, 0, 0);
            c.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            c.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 0.0]);
            if !inst.is_empty() {
                c.RSSetViewports(Some(&[D3D11_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: w as f32,
                    Height: h as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                }]));
                c.RSSetState(&self.raster);
                c.OMSetBlendState(&self.blend, None, 0xffff_ffff);
                c.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                c.IASetInputLayout(&self.layout);
                let stride = std::mem::size_of::<Inst>() as u32;
                let offset = 0u32;
                c.IASetVertexBuffers(0, 1, Some(&Some(self.vb.clone())), Some(&stride), Some(&offset));
                c.VSSetShader(&self.vs, None);
                c.PSSetShader(&self.ps, None);
                c.VSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
                c.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
                c.DrawInstanced(4, inst.len() as u32, 0, 0);
            }
            sc.Present(0, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

    /// Сбросить очередь команд и освободить память драйвера (при паузе).
    pub fn trim(&self) {
        unsafe {
            self.ctx.ClearState();
            self.ctx.Flush();
            if let Ok(d3) = self.device.cast::<IDXGIDevice3>() {
                d3.Trim();
            }
        }
    }
}

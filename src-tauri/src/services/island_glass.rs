//! GPU desktop backdrop, clipped to the capsule independently of the WebView surface.
//! Unlike whole-HWND Acrylic, the transparent envelope never receives a backdrop.
use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::Emitter;
static READY: AtomicBool = AtomicBool::new(false);
pub fn is_ready() -> bool {
    READY.load(Ordering::Relaxed)
}
use windows::{
    core::{implement, Interface, Result, GUID, HSTRING, PCWSTR},
    Foundation::{IPropertyValue, PropertyValue, TimeSpan},
    Graphics::Effects::{
        IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl,
    },
    System::DispatcherQueueController,
    Win32::{
        Foundation::{E_INVALIDARG, HWND},
        Graphics::{
            Direct2D::CLSID_D2D1GaussianBlur,
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_HOSTBACKDROPBRUSH},
        },
        System::WinRT::{
            Composition::ICompositorDesktopInterop,
            CreateDispatcherQueueController, DispatcherQueueOptions,
            Graphics::Direct2D::{
                IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
                GRAPHICS_EFFECT_PROPERTY_MAPPING, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
            },
            DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
        },
    },
    UI::Composition::{
        CompositionEffectSourceParameter, CompositionRoundedRectangleGeometry, Compositor,
        ContainerVisual, Desktop::DesktopWindowTarget,
    },
};
use windows_numerics::{Vector2, Vector3};

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct FrostEffect(IGraphicsEffectSource);
impl IGraphicsEffectSource_Impl for FrostEffect_Impl {}
impl IGraphicsEffect_Impl for FrostEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok("IslandFrost".into())
    }
    fn SetName(&self, _: &HSTRING) -> Result<()> {
        Ok(())
    }
}
impl IGraphicsEffectD2D1Interop_Impl for FrostEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }
    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }
    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingle(18.)?.cast(),
            1 | 2 => PropertyValue::CreateUInt32(1)?.cast(), // balanced optimization, hard border
            _ => Err(E_INVALIDARG.into()),
        }
    }
    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.0.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }
    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        if index.is_null() || mapping.is_null() {
            return Err(E_INVALIDARG.into());
        }
        // The compositor supplies the string and valid writable out parameters.
        let property = unsafe { name.to_string()? };
        let value = match property.as_str() {
            "StandardDeviation" => 0,
            "Optimization" => 1,
            "BorderMode" => 2,
            _ => return Err(E_INVALIDARG.into()),
        };
        unsafe {
            *index = value;
            *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
        }
        Ok(())
    }
}

struct Glass {
    _queue: DispatcherQueueController,
    _target: DesktopWindowTarget,
    compositor: Compositor,
    visual: ContainerVisual,
    shape: CompositionRoundedRectangleGeometry,
    initialized: bool,
}
thread_local! { static GLASS: RefCell<Option<Glass>> = const { RefCell::new(None) }; }

impl Glass {
    fn new(hwnd: HWND) -> Result<Self> {
        let enabled = 1i32;
        // Called on Tauri's UI thread, with a live window and stack-local BOOL.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&enabled as *const i32).cast(),
                4,
            )?;
        }
        let queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })
        }?;
        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        // The non-topmost composition slot sits below the WebView's child HWND.
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, false) }?;
        let root = compositor.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(Vector2 { X: 1., Y: 1. })?;
        let visual = compositor.CreateContainerVisual()?;
        visual.SetAnchorPoint(Vector2 { X: 0.5, Y: 0. })?;
        visual.SetRelativeOffsetAdjustment(Vector3 {
            X: 0.5,
            Y: 0.,
            Z: 0.,
        })?;
        let source = CompositionEffectSourceParameter::Create(&HSTRING::from("desktop"))?;
        let effect: IGraphicsEffect = FrostEffect(source.cast()?).into();
        let brush = compositor.CreateEffectFactory(&effect)?.CreateBrush()?;
        brush.SetSourceParameter(
            &HSTRING::from("desktop"),
            &compositor.CreateHostBackdropBrush()?,
        )?;
        let backdrop = compositor.CreateSpriteVisual()?;
        backdrop.SetRelativeSizeAdjustment(Vector2 { X: 1., Y: 1. })?;
        backdrop.SetBrush(&brush)?;
        // M3 surface tint matches --bg-surface. Both children share one animated clip.
        let tint = compositor.CreateSpriteVisual()?;
        tint.SetRelativeSizeAdjustment(Vector2 { X: 1., Y: 1. })?;
        tint.SetBrush(&compositor.CreateColorBrushWithColor(windows::UI::Color {
            A: 194,
            R: 22,
            G: 25,
            B: 35,
        })?)?;
        visual.Children()?.InsertAtBottom(&backdrop)?;
        visual.Children()?.InsertAtTop(&tint)?;
        let shape = compositor.CreateRoundedRectangleGeometry()?;
        visual.SetClip(&compositor.CreateGeometricClipWithGeometry(&shape)?)?;
        root.Children()?.InsertAtBottom(&visual)?;
        target.SetRoot(&root)?;
        Ok(Self {
            _queue: queue,
            _target: target,
            compositor,
            visual,
            shape,
            initialized: false,
        })
    }
    fn update(&mut self, width: f32, height: f32, radius: f32, animate: bool) -> Result<()> {
        let size = Vector2 {
            X: width,
            Y: height,
        };
        let corners = Vector2 {
            X: radius,
            Y: radius,
        };
        if animate && self.initialized {
            let easing = self.compositor.CreateCubicBezierEasingFunction(
                // Match the WebView capsule/subtitle spring (360 ms).
                Vector2 { X: 0.22, Y: 1.32 },
                Vector2 { X: 0.36, Y: 1. },
            )?;
            let size_animation = self.compositor.CreateVector2KeyFrameAnimation()?;
            size_animation.SetDuration(TimeSpan {
                Duration: 3_600_000,
            })?;
            size_animation.InsertKeyFrameWithEasingFunction(1., size, &easing)?;
            let radius_animation = self.compositor.CreateVector2KeyFrameAnimation()?;
            radius_animation.SetDuration(TimeSpan {
                Duration: 3_600_000,
            })?;
            radius_animation.InsertKeyFrameWithEasingFunction(1., corners, &easing)?;
            self.visual
                .StartAnimation(&HSTRING::from("Size"), &size_animation)?;
            self.shape
                .StartAnimation(&HSTRING::from("Size"), &size_animation)?;
            self.shape
                .StartAnimation(&HSTRING::from("CornerRadius"), &radius_animation)?;
        } else {
            self.visual.StopAnimation(&HSTRING::from("Size"))?;
            self.shape.StopAnimation(&HSTRING::from("Size"))?;
            self.shape.StopAnimation(&HSTRING::from("CornerRadius"))?;
            self.visual.SetSize(size)?;
            self.shape.SetSize(size)?;
            self.shape.SetCornerRadius(corners)?;
        }
        self.initialized = true;
        Ok(())
    }
}

pub fn update(window: &tauri::WebviewWindow, width: f32, height: f32, radius: f32, animate: bool) {
    let Ok(hwnd) = window.hwnd() else { return };
    let raw = hwnd.0 as isize;
    let events = window.clone();
    let _ = window.run_on_main_thread(move || {
        GLASS.with(|slot| {
            let result = (|| -> Result<()> {
                let mut glass = slot.borrow_mut();
                if glass.is_none() {
                    *glass = Some(Glass::new(HWND(raw as *mut _))?);
                }
                glass
                    .as_mut()
                    .unwrap()
                    .update(width, height, radius, animate)
            })();
            if result.is_ok() && !READY.swap(true, Ordering::Relaxed) {
                let _ = events.emit("island:glass", true);
            }
            if let Err(error) = result {
                eprintln!("Island desktop glass: {error}");
            }
        });
    });
}

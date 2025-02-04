use core::{
    fmt,
};
use alloc::vec::Vec;
use alloc::collections::btree_map::BTreeMap;
use azul_css::{
    LayoutPoint, LayoutSize, LayoutRect,
    StyleBackgroundRepeat, StyleBackgroundPosition, ColorU,
    LinearGradient, RadialGradient, ConicGradient, StyleBoxShadow, StyleBackgroundSize,
    CssPropertyValue, BoxShadowClipMode,

    LayoutBorderTopWidth, LayoutBorderRightWidth, LayoutBorderBottomWidth, LayoutBorderLeftWidth,
    StyleBorderTopColor, StyleBorderRightColor, StyleBorderBottomColor, StyleBorderLeftColor,
    StyleBorderTopStyle, StyleBorderRightStyle, StyleBorderBottomStyle, StyleBorderLeftStyle,
    StyleBorderTopLeftRadius, StyleBorderTopRightRadius, StyleBorderBottomLeftRadius, StyleBorderBottomRightRadius,
};
use crate::{
    callbacks::{DocumentId, PipelineId, DomNodeId},
    ui_solver::{ExternalScrollId, LayoutResult, PositionInfo, ComputedTransform3D},
    window::{FullWindowState, LogicalRect, LogicalPosition, LogicalSize},
    app_resources::{
        ImageCache, RendererResources, AddImageMsg, ImageDescriptor,
        ImageKey, FontInstanceKey, PrimitiveFlags,
        Epoch, ExternalImageId, GlyphOptions, LoadFontFn, ParseFontFn,
        ResourceUpdate, IdNamespace, TransformKey, OpacityKey,
    },
    styled_dom::{DomId, AzNodeId, StyledDom, ContentGroup},
    id_tree::NodeId,
    dom::{TagId, ScrollTagId},
};
use crate::gl::{Texture, OptionGlContextPtr};
use rust_fontconfig::FcFontCache;

pub type GlyphIndex = u32;

#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd)]
pub struct GlyphInstance {
    pub index: GlyphIndex,
    pub point: LogicalPosition,
    pub size: LogicalSize,
}

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub struct DisplayListImageMask {
    pub image: ImageKey,
    pub rect: LogicalRect,
    pub repeat: bool,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct CachedDisplayList {
    pub root: DisplayListMsg,
    pub root_size: LogicalSize,
}

impl CachedDisplayList {
    pub fn empty() -> Self {
        Self {
            root: DisplayListMsg::Frame(DisplayListFrame::root(LayoutSize::zero(), LayoutPoint::zero())),
            root_size: LogicalSize::zero(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum DisplayListMsg {
    // nested display list
    IFrame(PipelineId, LogicalSize, Epoch, Box<CachedDisplayList>),
    Frame(DisplayListFrame),
    ScrollFrame(DisplayListScrollFrame),
}

impl DisplayListMsg {

    pub fn get_transform_key(&self) -> Option<&(TransformKey, ComputedTransform3D)> {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => f.transform.as_ref(),
            ScrollFrame(sf) => sf.frame.transform.as_ref(),
            IFrame(_, _, _, _) => None,
        }
    }

    pub fn get_opacity_key(&self) -> Option<&(OpacityKey, f32)> {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => f.opacity.as_ref(),
            ScrollFrame(sf) => sf.frame.opacity.as_ref(),
            IFrame(_, _, _, _) => None,
        }
    }

    pub fn get_image_mask(&self) -> Option<&DisplayListImageMask> {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => f.clip_mask.as_ref(),
            ScrollFrame(sf) => sf.frame.clip_mask.as_ref(),
            IFrame(_, _, _, _) => None,
        }
    }

    pub fn get_position(&self) -> PositionInfo {
        use self::DisplayListMsg::*;
        use crate::ui_solver::PositionInfoInner;
        match self {
            Frame(f) => f.position.clone(),
            ScrollFrame(sf) => sf.frame.position.clone(),
            IFrame(_, _, _, _) => PositionInfo::Static(PositionInfoInner::zero()),
        }
    }

    pub fn is_content_empty(&self) -> bool {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => { f.content.is_empty() },
            ScrollFrame(sf) => { sf.frame.content.is_empty() },
            IFrame(_, _, _, _) => false,
        }
    }

    pub fn has_no_children(&self) -> bool {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => { f.children.is_empty() },
            ScrollFrame(sf) => { sf.frame.children.is_empty() },
            IFrame(_, _, _, _) => false,
        }
    }

    pub fn push_content(&mut self, content: LayoutRectContent) {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => { f.content.push(content); },
            ScrollFrame(sf) => { sf.frame.content.push(content); },
            IFrame(_, _, _, _) => { } // invalid
        }
    }

    pub fn append_child(&mut self, child: Self) {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => { f.children.push(child); },
            ScrollFrame(sf) => { sf.frame.children.push(child); },
            IFrame(_, _, _, _) => { } // invalid
        }
    }

    pub fn append_children(&mut self, mut children: Vec<Self>) {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => { f.children.append(&mut children); },
            ScrollFrame(sf) => { sf.frame.children.append(&mut children); },
            IFrame(_, _, _, _) => { } // invalid
        }
    }

    pub fn get_size(&self) -> LogicalSize {
        use self::DisplayListMsg::*;
        match self {
            Frame(f) => f.size,
            ScrollFrame(sf) => sf.frame.size,
            IFrame(_, s, _, _) => *s,
        }
    }
}

#[derive(Clone, PartialEq, PartialOrd)]
pub struct DisplayListScrollFrame {
    /// Containing rect of the parent node
    pub parent_rect: LogicalRect,
    /// Bounding rect of the (overflowing) content of the scroll frame
    pub content_rect: LogicalRect,
    /// The scroll ID is the hash of the DOM node, so that scrolling
    /// positions can be tracked across multiple frames
    pub scroll_id: ExternalScrollId,
    /// The scroll tag is used for hit-testing
    pub scroll_tag: ScrollTagId,
    /// Content + children of the scroll clip
    pub frame: DisplayListFrame,
}

impl fmt::Debug for DisplayListScrollFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DisplayListScrollFrame {{\r\n")?;
        write!(f, "    parent_rect: {}\r\n", self.parent_rect)?;
        write!(f, "    content_rect: {}\r\n", self.content_rect)?;
        write!(f, "    scroll_tag: {}\r\n", self.scroll_tag)?;
        write!(f, "    frame: DisplayListFrame {{\r\n")?;
        let frame = format!("{:#?}", self.frame);
        let frame = frame.lines().map(|l| format!("        {}", l)).collect::<Vec<_>>().join("\r\n");
        write!(f, "{}\r\n", frame)?;
        write!(f, "    }}\r\n")?;
        write!(f, "}}")?;
        Ok(())
    }
}

#[derive(Clone, PartialEq, PartialOrd)]
pub struct DisplayListFrame {
    pub size: LogicalSize,
    pub position: PositionInfo,
    pub flags: PrimitiveFlags,
    pub clip_children: Option<LogicalSize>,
    pub clip_mask: Option<DisplayListImageMask>,
    /// Border radius, set to none only if overflow: visible is set!
    pub border_radius: StyleBorderRadius,
    pub tag: Option<TagId>,
    // box shadow has to be pushed twice: once as inset and once as outset
    pub box_shadow: Option<BoxShadow>,
    pub transform: Option<(TransformKey, ComputedTransform3D)>,
    pub opacity: Option<(OpacityKey, f32)>,
    pub content: Vec<LayoutRectContent>,
    pub children: Vec<DisplayListMsg>,
}

impl fmt::Debug for DisplayListFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let print_no_comma_rect =
            !self.border_radius.is_none() ||
            self.tag.is_some() ||
            !self.content.is_empty() ||
            !self.children.is_empty();

        write!(f, "rect: {:#?} @ {:?}{}", self.size, self.position, if !print_no_comma_rect { "" } else { "," })?;

        if !self.border_radius.is_none() {
            write!(f, "\r\nborder_radius: {:#?}", self.border_radius)?;
        }
        if let Some(tag) = &self.tag {
            write!(f, "\r\ntag: {}", tag.0)?;
        }
        if !self.content.is_empty() {
            write!(f, "\r\ncontent: {:#?}", self.content)?;
        }
        if !self.children.is_empty() {
            write!(f, "\r\nchildren: {:#?}", self.children)?;
        }

        Ok(())
    }
}

impl DisplayListFrame {
    pub fn root(dimensions: LayoutSize, root_origin: LayoutPoint) -> Self {
        use crate::ui_solver::PositionInfoInner;
        DisplayListFrame {
            tag: None,
            size: LogicalSize::new(dimensions.width as f32, dimensions.height as f32),
            clip_children: None,
            position: PositionInfo::Static(PositionInfoInner {
                x_offset: root_origin.x as f32,
                y_offset: root_origin.y as f32,
                static_x_offset: root_origin.x as f32,
                static_y_offset: root_origin.y as f32
            }),
            flags: PrimitiveFlags {
                is_backface_visible: true,
                is_scrollbar_container: false,
                is_scrollbar_thumb: false,
                prefer_compositor_surface: true,
                supports_external_compositor_surface: true,
            },
            border_radius: StyleBorderRadius::default(),
            box_shadow: None,
            transform: None,
            opacity: None,
            content: vec![],
            children: vec![],
            clip_mask: None,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageRendering {
    Auto,
    CrispEdges,
    Pixelated,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AlphaType {
    Alpha,
    PremultipliedAlpha,
}

#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleBorderRadius {
    pub top_left: Option<CssPropertyValue<StyleBorderTopLeftRadius>>,
    pub top_right: Option<CssPropertyValue<StyleBorderTopRightRadius>>,
    pub bottom_left: Option<CssPropertyValue<StyleBorderBottomLeftRadius>>,
    pub bottom_right: Option<CssPropertyValue<StyleBorderBottomRightRadius>>,
}

impl StyleBorderRadius {
    pub fn is_none(&self) -> bool {
        self.top_left.is_none() &&
        self.top_right.is_none() &&
        self.bottom_left.is_none() &&
        self.bottom_right.is_none()
    }
}
impl fmt::Debug for StyleBorderRadius {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "StyleBorderRadius {{")?;
        if let Some(tl) = &self.top_left {
            write!(f, "\r\n\ttop-left: {:?},", tl)?;
        }
        if let Some(tr) = &self.top_right {
            write!(f, "\r\n\ttop-right: {:?},", tr)?;
        }
        if let Some(bl) = &self.bottom_left {
            write!(f, "\r\n\tbottom-left: {:?},", bl)?;
        }
        if let Some(br) = &self.bottom_right {
            write!(f, "\r\n\tbottom-right: {:?},", br)?;
        }
        write!(f, "\r\n}}")
    }
}

macro_rules! tlbr_debug {($struct_name:ident) => (
    impl fmt::Debug for $struct_name {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{} {{", stringify!($struct_name))?;
            if let Some(t) = &self.top {
                write!(f, "\r\n\ttop: {:?},", t)?;
            }
            if let Some(r) = &self.right {
                write!(f, "\r\n\tright: {:?},", r)?;
            }
            if let Some(b) = &self.bottom {
                write!(f, "\r\n\tbottom: {:?},", b)?;
            }
            if let Some(l) = &self.left {
                write!(f, "\r\n\tleft: {:?},", l)?;
            }
            write!(f, "\r\n}}")
        }
    }
)}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleBorderWidths {
    pub top: Option<CssPropertyValue<LayoutBorderTopWidth>>,
    pub right: Option<CssPropertyValue<LayoutBorderRightWidth>>,
    pub bottom: Option<CssPropertyValue<LayoutBorderBottomWidth>>,
    pub left: Option<CssPropertyValue<LayoutBorderLeftWidth>>,
}

impl StyleBorderWidths {

    #[inline]
    pub fn left_width(&self) -> f32 {
        self.left.unwrap_or_default().get_property_owned().unwrap_or_default().inner.to_pixels(0.0)
    }

    #[inline]
    pub fn right_width(&self) -> f32 {
        self.right.unwrap_or_default().get_property_owned().unwrap_or_default().inner.to_pixels(0.0)
    }

    #[inline]
    pub fn top_width(&self) -> f32 {
        self.top.unwrap_or_default().get_property_owned().unwrap_or_default().inner.to_pixels(0.0)
    }

    #[inline]
    pub fn bottom_width(&self) -> f32 {
        self.bottom.unwrap_or_default().get_property_owned().unwrap_or_default().inner.to_pixels(0.0)
    }

    #[inline]
    pub fn total_horizontal(&self) -> f32 {
        self.left_width() + self.right_width()
    }

    #[inline]
    pub fn total_vertical(&self) -> f32 {
        self.top_width() + self.bottom_width()
    }
}

tlbr_debug!(StyleBorderWidths);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleBorderColors {
    pub top: Option<CssPropertyValue<StyleBorderTopColor>>,
    pub right: Option<CssPropertyValue<StyleBorderRightColor>>,
    pub bottom: Option<CssPropertyValue<StyleBorderBottomColor>>,
    pub left: Option<CssPropertyValue<StyleBorderLeftColor>>,
}

tlbr_debug!(StyleBorderColors);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleBorderStyles {
    pub top: Option<CssPropertyValue<StyleBorderTopStyle>>,
    pub right: Option<CssPropertyValue<StyleBorderRightStyle>>,
    pub bottom: Option<CssPropertyValue<StyleBorderBottomStyle>>,
    pub left: Option<CssPropertyValue<StyleBorderLeftStyle>>,
}

tlbr_debug!(StyleBorderStyles);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoxShadow {
    pub clip_mode: BoxShadowClipMode,
    pub top: Option<CssPropertyValue<StyleBoxShadow>>,
    pub right: Option<CssPropertyValue<StyleBoxShadow>>,
    pub bottom: Option<CssPropertyValue<StyleBoxShadow>>,
    pub left: Option<CssPropertyValue<StyleBoxShadow>>,
}

tlbr_debug!(BoxShadow);

#[derive(Clone, PartialEq, PartialOrd)]
pub enum LayoutRectContent {
    Text {
        glyphs: Vec<GlyphInstance>,
        font_instance_key: FontInstanceKey,
        color: ColorU,
        glyph_options: Option<GlyphOptions>,
        overflow: (bool, bool),
    },
    Background {
        content: RectBackground,
        size: Option<StyleBackgroundSize>,
        offset: Option<StyleBackgroundPosition>,
        repeat: Option<StyleBackgroundRepeat>,
    },
    Image {
        size: LogicalSize,
        offset: LogicalPosition,
        image_rendering: ImageRendering,
        alpha_type: AlphaType,
        image_key: ImageKey,
        background_color: ColorU,
    },
    Border {
        widths: StyleBorderWidths,
        colors: StyleBorderColors,
        styles: StyleBorderStyles,
    },
}

impl fmt::Debug for LayoutRectContent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::LayoutRectContent::*;
        match self {
            Text { glyphs, font_instance_key, color, glyph_options, overflow } => {
                let glyphs_str = glyphs.iter().map(|g| format!("        {:?}", g)).collect::<Vec<_>>().join(",\r\n");
                write!(f,
                    "Text {{\r\n\
                       .    glyphs: [\r\n{}\r\n],\r\n\
                       .    font_instance_key: {:?},\r\n\
                       .    color: {},\r\n\
                       .    glyph_options: {:?},\r\n\
                       .    overflow: {:?},\r\n\
                    }}",
                    glyphs_str, font_instance_key.key, color, glyph_options, overflow
                )
            },
            Background { content, size, offset, repeat } => {
                write!(f, "Background {{\r\n")?;
                write!(f, "    content: {:?},\r\n", content)?;
                write!(f, "    size: {:?},\r\n", size)?;
                write!(f, "    offset: {:?},\r\n", offset)?;
                write!(f, "    repeat: {:?},\r\n", repeat)?;
                write!(f, "}}")
            },
            Image { size, offset, image_rendering, alpha_type, image_key, background_color } => {
                write!(f,
                    "Image {{\r\n\
                        size: {:?},\r\n\
                        offset: {:?},\r\n\
                        image_rendering: {:?},\r\n\
                        alpha_type: {:?},\r\n\
                        image_key: {:?},\r\n\
                        background_color: {:?}\r\n\
                    }}",
                    size, offset, image_rendering, alpha_type, image_key, background_color
                )
            },
            Border { widths, colors, styles, } => {
                write!(f,
                    "Border {{\r\n\
                        widths: {:?},\r\n\
                        colors: {:?},\r\n\
                        styles: {:?}\r\n\
                    }}",
                    widths, colors, styles,
                )
            }
        }
    }
}

#[derive(Clone, PartialEq, PartialOrd)]
pub enum RectBackground {
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
    ConicGradient(ConicGradient),
    Image((ImageKey, ImageDescriptor)),
    Color(ColorU),
}

impl fmt::Debug for RectBackground {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::RectBackground::*;
        match self {
            LinearGradient(l) => write!(f, "{:?}", l),
            RadialGradient(r) => write!(f, "{:?}", r),
            ConicGradient(c) => write!(f, "{:?}", c),
            Image(id) => write!(f, "image({:#?})", id),
            Color(c) => write!(f, "{}", c),
        }
    }
}

impl RectBackground {
    pub fn get_content_size(&self) -> Option<(f32, f32)> {
        match self {
            RectBackground::Image((_key, descriptor)) => {
                Some((descriptor.width as f32, descriptor.height as f32))
            }
            _ => None,
        }
    }
}

// ------------------- NEW DISPLAY LIST CODE

/// Since the display list can take a lot of parameters, we don't want to
/// continually pass them as parameters of the function and rather use a
/// struct to pass them around. This is purely for ergonomic reasons.
///
/// `DisplayListParametersRef` has only members that are
///  **immutable references** to other things that need to be passed down the display list
#[derive(Clone)]
pub struct DisplayListParametersRef<'a> {
    /// ID of this Dom
    pub dom_id: DomId,
    /// Document ID (window ID)
    pub document_id: &'a DocumentId,
    /// Epoch of all the OpenGL textures
    pub epoch: Epoch,
    /// The CSS that should be applied to the DOM
    pub full_window_state: &'a FullWindowState,
    /// Cached layouts (+ solved layouts for iframes)
    pub layout_results: &'a [LayoutResult],
    /// Cached rendered OpenGL textures
    pub gl_texture_cache: &'a GlTextureCache,
    /// Cached IDs for CSS backgrounds
    pub image_cache: &'a ImageCache,
    /// Reference to the RendererResources, necessary to query info about image and font keys
    pub renderer_resources: &'a RendererResources,
}

#[derive(Debug, Default)]
pub struct GlTextureCache {
    pub solved_textures: BTreeMap<DomId, BTreeMap<NodeId, (ImageKey, ImageDescriptor)>>,
}

unsafe impl Send for GlTextureCache { } // necessary so the display list can be built in parallel

// todo: very unclean
pub type LayoutFn = fn(StyledDom, &ImageCache, &FcFontCache, &mut RendererResources, &mut Vec<ResourceUpdate>, IdNamespace, &DocumentId, Epoch, &RenderCallbacks, &FullWindowState) -> Vec<LayoutResult>;
pub type GlStoreImageFn = fn(DocumentId, Epoch, Texture) -> ExternalImageId;

#[derive(Debug, Default)]
pub struct SolvedLayout {
    pub layout_results: Vec<LayoutResult>,
}

#[derive(Clone)]
pub struct RenderCallbacks {
    pub insert_into_active_gl_textures_fn: GlStoreImageFn,
    pub layout_fn: LayoutFn,
    pub load_font_fn: LoadFontFn,
    pub parse_font_fn: ParseFontFn,
}

impl GlTextureCache {

    pub fn empty() -> Self {
        Self {
            solved_textures: BTreeMap::new(),
        }
    }

    /// Invokes all ImageCallbacks with the sizes given by the LayoutResult
    /// and adds them to the renderer resources.
    pub fn new(
        layout_results: &mut [LayoutResult],
        gl_context: &OptionGlContextPtr,
        id_namespace: IdNamespace,
        document_id: &DocumentId,
        epoch: Epoch,
        hidpi_factor: f32,
        image_cache: &ImageCache,
        system_fonts: &FcFontCache,
        callbacks: &RenderCallbacks,
        all_resource_updates: &mut Vec<ResourceUpdate>,
        renderer_resources: &mut RendererResources,
    ) -> Self {

        use crate::{
            app_resources::{
                AddImage, ExternalImageData, ImageBufferKind, ExternalImageType,
                ImageData, add_resources, DecodedImage, ImageRef,
            },
            callbacks::{RenderImageCallbackInfo, HidpiAdjustedBounds},
            dom::NodeType,
        };
        use gl_context_loader::gl;

        let mut solved_image_callbacks = BTreeMap::new();

        // Now that the layout is done, render the OpenGL textures and add them to the RenderAPI
        for (dom_id, layout_result) in layout_results.iter_mut().enumerate() {
            for callback_node_id in layout_result.styled_dom.scan_for_gltexture_callbacks() {

                // Invoke OpenGL callback, render texture
                let rect_size = layout_result.rects.as_ref()[callback_node_id].size;

                let callback_image = {

                    let callback_domnode_id = DomNodeId {
                        dom: DomId { inner: dom_id },
                        node: AzNodeId::from_crate_internal(Some(callback_node_id)),
                    };

                    let size = LayoutSize::new(
                        rect_size.width.round() as isize,
                        rect_size.height.round() as isize
                    );

                    // NOTE: all of these extra arguments are necessary so that the callback
                    // has access to information about the text layout, which is used to render
                    // the "text selection" highlight (the text selection is nothing but an image
                    // or an image mask).
                    let gl_callback_info = RenderImageCallbackInfo::new(
                        /*gl_context:*/ &gl_context,
                        /*image_cache:*/ image_cache,
                        /*system_fonts:*/ system_fonts,
                        /*node_hierarchy*/ &layout_result.styled_dom.node_hierarchy,
                        /*words_cache*/ &layout_result.words_cache,
                        /*shaped_words_cache*/ &layout_result.shaped_words_cache,
                        /*positioned_words_cache*/ &layout_result.positioned_words_cache,
                        /*positioned_rects*/ &layout_result.rects,
                        /*bounds:*/ HidpiAdjustedBounds::from_bounds(size, hidpi_factor),
                        /*hit_dom_node*/ callback_domnode_id,
                    );

                    let callback_image: Option<ImageRef> = {
                        // get a MUTABLE reference to the RefAny inside of the DOM
                        let mut node_data_mut = layout_result.styled_dom.node_data.as_container_mut();
                        match &mut node_data_mut[callback_node_id].node_type {
                            NodeType::Image(img) => {
                                img
                                .get_image_callback_mut()
                                .map(|gl_texture_callback| {
                                    (gl_texture_callback.callback.cb)(&mut gl_texture_callback.data, gl_callback_info)
                                })
                            },
                            _ => None,
                        }
                    };

                    // Reset the framebuffer and SRGB color target to 0
                    if let Some(gl) = gl_context.as_ref() {
                        gl.bind_framebuffer(gl::FRAMEBUFFER, 0);
                        gl.disable(gl::FRAMEBUFFER_SRGB);
                        gl.disable(gl::MULTISAMPLE);
                    }

                    callback_image
                };

                if let Some(image_ref) = callback_image {
                    solved_image_callbacks
                        .entry(layout_result.dom_id.clone())
                        .or_insert_with(|| BTreeMap::default())
                        .insert(callback_node_id, image_ref);
                }
            }
        }

        let mut image_resource_updates = Vec::new();
        let mut gl_texture_cache = Self::empty();

        for (dom_id, image_refs) in solved_image_callbacks {
            for (node_id, image_ref) in image_refs {

                let image_ref_hash = image_ref.get_hash();
                let image_data = match image_ref.into_inner() {
                    Some(s) => s,
                    None => continue,
                };
                let image_result = match image_data {
                    DecodedImage::Gl(texture) => {
                        let descriptor = texture.get_descriptor();
                        let key = ImageKey::unique(id_namespace);
                        let external_image_id = (callbacks.insert_into_active_gl_textures_fn)(*document_id, epoch, texture);

                        gl_texture_cache.solved_textures
                            .entry(dom_id.clone())
                            .or_insert_with(|| BTreeMap::new())
                            .insert(node_id, (key, descriptor));

                        Some((image_ref_hash, AddImageMsg(
                            AddImage {
                                key,
                                data: ImageData::External(ExternalImageData {
                                    id: external_image_id,
                                    channel_index: 0,
                                    image_type: ExternalImageType::TextureHandle(ImageBufferKind::Texture2D),
                                }),
                                descriptor,
                                tiling: None,
                            }
                        )))
                    },
                    DecodedImage::Raw((descriptor, data)) => {
                        let key = ImageKey::unique(id_namespace);
                        Some((image_ref_hash, AddImageMsg(AddImage {
                            key,
                            data: data,
                            descriptor: descriptor,
                            tiling: None
                        })))
                    },
                    DecodedImage::NullImage { width: _, height: _, format: _ } => None,
                    // Texture callbacks inside of texture callbacks are not rendered
                    DecodedImage::Callback(_) => None,
                };

                if let Some((image_ref_hash, add_img_msg)) = image_result {
                    image_resource_updates.push((image_ref_hash, add_img_msg));
                }
            }
        }

        // Add the new rendered images to the RenderApi
        add_resources(
            renderer_resources,
            all_resource_updates,
            Vec::new(),
            image_resource_updates
        );

        gl_texture_cache
    }
}

impl SolvedLayout {

    /// Does the layout, updates the image + font resources for the RenderAPI
    #[cfg(feature = "multithreading")]
    pub fn new(
        styled_dom: StyledDom,
        epoch: Epoch,
        document_id: &DocumentId,
        full_window_state: &FullWindowState,
        all_resource_updates: &mut Vec<ResourceUpdate>,
        id_namespace: IdNamespace,
        image_cache: &ImageCache,
        system_fonts: &FcFontCache,
        callbacks: &RenderCallbacks,
        renderer_resources: &mut RendererResources,
    ) -> Self {
        Self {
            layout_results: (callbacks.layout_fn)(
                styled_dom,
                image_cache,
                system_fonts,
                renderer_resources,
                all_resource_updates,
                id_namespace,
                document_id,
                epoch,
                callbacks,
                &full_window_state,
            )
        }
    }
}

#[cfg(feature = "multithreading")]
pub fn push_rectangles_into_displaylist<'a>(
    root_content_group: &ContentGroup,
    referenced_content: &DisplayListParametersRef<'a>,
) -> Option<DisplayListMsg> {

    use rayon::prelude::*;

    let mut content = displaylist_handle_rect(
        root_content_group.root.into_crate_internal().unwrap(),
        referenced_content,
    )?;

    let children = root_content_group.children
        .as_ref()
        .par_iter()
        .filter_map(|child_content_group| {
            push_rectangles_into_displaylist(
                child_content_group,
                referenced_content,
            )
        })
        .collect();

    content.append_children(children);

    Some(content)
}

/// Push a single rectangle into the display list builder
#[cfg(feature = "multithreading")]
pub fn displaylist_handle_rect<'a>(
    rect_idx: NodeId,
    referenced_content: &DisplayListParametersRef<'a>,
) -> Option<DisplayListMsg> {

    use crate::dom::NodeType::*;
    use crate::styled_dom::AzTagId;
    use azul_css::LayoutDisplay;

    let DisplayListParametersRef {
        dom_id,
        layout_results,
        gl_texture_cache,
        renderer_resources,
        image_cache,
        ..
    } = referenced_content;

    let layout_result = &layout_results[dom_id.inner];
    let styled_node = &layout_result.styled_dom.styled_nodes.as_container()[rect_idx];
    let positioned_rect = &layout_result.rects.as_ref()[rect_idx];
    let html_node = &layout_result.styled_dom.node_data.as_container()[rect_idx];

    let tag_id = styled_node.tag_id.into_option().or({
        layout_result.scrollable_nodes.overflowing_nodes
        .get(&AzNodeId::from_crate_internal(Some(rect_idx)))
        .map(|scrolled| AzTagId::from_crate_internal(scrolled.scroll_tag_id.0))
    });

    let clip_mask = html_node.get_clip_mask().and_then(|m| {
        let clip_mask_hash = m.image.get_hash();
        let (image_key, _) = renderer_resources.currently_registered_images.get(&clip_mask_hash)?;
        Some(DisplayListImageMask {
            image: *image_key,
            rect: m.rect,
            repeat: m.repeat,
        })
    });

    // do not push display:none items in any way
    //
    // TODO: this currently operates on the visual order, not on the DOM order!
    let display = layout_result.styled_dom.get_css_property_cache()
        .get_display(&html_node, &rect_idx, &styled_node.state)
        .cloned()
        .unwrap_or_default();

    if display == CssPropertyValue::None ||
       display == CssPropertyValue::Exact(LayoutDisplay::None) {
        return None;
    }

    let mut frame = DisplayListFrame {
        tag: tag_id.map(|t| t.into_crate_internal()),
        size: positioned_rect.size,
        clip_children: layout_result.scrollable_nodes.clip_nodes.get(&rect_idx).copied(),
        position: positioned_rect.position.clone(),
        border_radius: StyleBorderRadius {
            top_left: layout_result.styled_dom.get_css_property_cache()
                .get_border_top_left_radius(&html_node, &rect_idx, &styled_node.state).cloned(),
            top_right: layout_result.styled_dom.get_css_property_cache()
                .get_border_top_right_radius(&html_node, &rect_idx, &styled_node.state).cloned(),
            bottom_left: layout_result.styled_dom.get_css_property_cache()
                .get_border_bottom_left_radius(&html_node, &rect_idx, &styled_node.state).cloned(),
            bottom_right: layout_result.styled_dom.get_css_property_cache()
                .get_border_bottom_right_radius(&html_node, &rect_idx, &styled_node.state).cloned(),
        },
        flags: PrimitiveFlags {
            is_backface_visible: false, // TODO!
            is_scrollbar_container: false,
            is_scrollbar_thumb: false,
            prefer_compositor_surface: false,
            supports_external_compositor_surface: false,
        },
        content: Vec::new(),
        children: Vec::new(),
        box_shadow: None,
        transform: layout_result.gpu_value_cache.transform_keys
            .get(&rect_idx)
            .and_then(|key| Some((*key, layout_result.gpu_value_cache.current_transform_values.get(&rect_idx).cloned()?))),
        opacity: layout_result.gpu_value_cache.opacity_keys
            .get(&rect_idx)
            .and_then(|key| Some((*key, layout_result.gpu_value_cache.current_opacity_values.get(&rect_idx).cloned()?))),
        clip_mask,
    };

    // push box shadow
    let box_shadow_left = layout_result.styled_dom.get_css_property_cache().get_box_shadow_left(&html_node, &rect_idx, &styled_node.state);
    let box_shadow_right = layout_result.styled_dom.get_css_property_cache().get_box_shadow_right(&html_node, &rect_idx, &styled_node.state);
    let box_shadow_top = layout_result.styled_dom.get_css_property_cache().get_box_shadow_top(&html_node, &rect_idx, &styled_node.state);
    let box_shadow_bottom = layout_result.styled_dom.get_css_property_cache().get_box_shadow_bottom(&html_node, &rect_idx, &styled_node.state);

    let box_shadows = [&box_shadow_left, &box_shadow_right, &box_shadow_top, &box_shadow_bottom];

    let box_shadow = if box_shadows.iter().all(|b| b.is_some()) {
        let mut clip_mode = None;

        if box_shadows.iter().all(|b| b.and_then(|b| b.get_property().map(|p| p.clip_mode)) == Some(BoxShadowClipMode::Outset)) {
            clip_mode = Some(BoxShadowClipMode::Outset);
        } else if box_shadows.iter().all(|b| b.and_then(|b| b.get_property().map(|p| p.clip_mode)) == Some(BoxShadowClipMode::Inset)) {
            clip_mode = Some(BoxShadowClipMode::Inset);
        }

        clip_mode.map(|c| BoxShadow {
            clip_mode: c,
            left: box_shadow_left.cloned(),
            right: box_shadow_right.cloned(),
            top: box_shadow_top.cloned(),
            bottom: box_shadow_bottom.cloned(),
        })
    } else {
        None
    };

    frame.box_shadow = box_shadow;

    // push background
    let bg_opt = layout_result.styled_dom.get_css_property_cache()
    .get_background_content(&html_node, &rect_idx, &styled_node.state);

    if let Some(bg) = bg_opt.as_ref().and_then(|br| br.get_property()) {

        use azul_css::{StyleBackgroundSizeVec, StyleBackgroundPositionVec, StyleBackgroundRepeatVec};

        let default_bg_size_vec: StyleBackgroundSizeVec = Vec::new().into();
        let default_bg_position_vec: StyleBackgroundPositionVec = Vec::new().into();
        let default_bg_repeat_vec: StyleBackgroundRepeatVec = Vec::new().into();

        let bg_sizes_opt = layout_result.styled_dom.get_css_property_cache().get_background_size(&html_node, &rect_idx, &styled_node.state);
        let bg_positions_opt = layout_result.styled_dom.get_css_property_cache().get_background_position(&html_node, &rect_idx, &styled_node.state);
        let bg_repeats_opt = layout_result.styled_dom.get_css_property_cache().get_background_repeat(&html_node, &rect_idx, &styled_node.state);

        let bg_sizes = bg_sizes_opt.as_ref().and_then(|p| p.get_property()).unwrap_or(&default_bg_size_vec);
        let bg_positions = bg_positions_opt.as_ref().and_then(|p| p.get_property()).unwrap_or(&default_bg_position_vec);
        let bg_repeats = bg_repeats_opt.as_ref().and_then(|p| p.get_property()).unwrap_or(&default_bg_repeat_vec);

        for (bg_index, bg) in bg.iter().enumerate() {

            use azul_css::StyleBackgroundContent::*;
            use azul_css::AzString;

            fn get_image_background_key(
                renderer_resources: &RendererResources,
                image_cache: &ImageCache,
                background_image_id: &AzString,
            ) -> Option<(ImageKey, ImageDescriptor)> {
                let image_ref = image_cache.get_css_image_id(background_image_id)?;
                let image_ref_hash = image_ref.get_hash();
                let (image_key, image_descriptor) = renderer_resources.currently_registered_images.get(&image_ref_hash)?;
                Some((*image_key, image_descriptor.clone()))
            }

            let background_content = match bg {
                LinearGradient(lg) => Some(RectBackground::LinearGradient(lg.clone())),
                RadialGradient(rg) => Some(RectBackground::RadialGradient(rg.clone())),
                ConicGradient(cg) => Some(RectBackground::ConicGradient(cg.clone())),
                Image(i) => get_image_background_key(&renderer_resources, &image_cache, i).map(RectBackground::Image),
                Color(c) => Some(RectBackground::Color(*c)),
            };

            let bg_size = bg_sizes.get(bg_index).or(bg_sizes.get(0)).copied();
            let bg_position = bg_positions.get(bg_index).or(bg_positions.get(0)).copied();
            let bg_repeat = bg_repeats.get(bg_index).or(bg_repeats.get(0)).copied();

            if let Some(background_content) = background_content {
                frame.content.push(LayoutRectContent::Background {
                    content: background_content,
                    size: bg_size.clone(),
                    offset: bg_position.clone(),
                    repeat: bg_repeat.clone(),
                });
            }
        }
    }

    match html_node.get_node_type() {
        Div | Body | Br => { },
        Text(_) => {

            use crate::app_resources::get_inline_text;

            // compute the layouted glyphs here, this way it's easier
            // to reflow text since there is no cache that needs to be updated
            //
            // if the text is reflowed, the display list needs to update anyway
            if let (Some(words), Some(shaped_words), Some(word_positions), Some((_, inline_text_layout))) = (
                layout_result.words_cache.get(&rect_idx),
                layout_result.shaped_words_cache.get(&rect_idx),
                layout_result.positioned_words_cache.get(&rect_idx),
                positioned_rect.resolved_text_layout_options.as_ref(),
            ) {

                let inline_text = get_inline_text(&words, &shaped_words, &word_positions.0, &inline_text_layout);
                let layouted_glyphs = inline_text.get_layouted_glyphs();

                if !layouted_glyphs.glyphs.is_empty() {

                    let font_instance_key = word_positions.1;
                    let text_color = layout_result.styled_dom.get_css_property_cache()
                    .get_text_color_or_default(&html_node, &rect_idx, &styled_node.state);
                    let overflow_horizontal_visible = layout_result.styled_dom.get_css_property_cache()
                    .is_horizontal_overflow_visible(&html_node, &rect_idx, &styled_node.state);
                    let overflow_vertical_visible = layout_result.styled_dom.get_css_property_cache()
                    .is_vertical_overflow_visible(&html_node, &rect_idx, &styled_node.state);

                    frame.content.push(LayoutRectContent::Text {
                       glyphs: layouted_glyphs.glyphs,
                       font_instance_key,
                       color: text_color.inner,
                       glyph_options: None,
                       overflow: (overflow_horizontal_visible, overflow_vertical_visible),
                    });
                }
            }
        },
        Image(image_ref) => {
            use crate::app_resources::DecodedImage;

            let image_hash = image_ref.get_hash();
            let image_size = image_ref.get_size();

            match image_ref.get_data() {
                DecodedImage::NullImage { .. } => {
                    frame.content.push(LayoutRectContent::Image {
                        size: image_size,
                        offset: LogicalPosition::zero(),
                        image_rendering: ImageRendering::Auto,
                        alpha_type: AlphaType::Alpha,
                        image_key: ImageKey::DUMMY,
                        background_color: ColorU::WHITE,
                    })
                },
                DecodedImage::Gl(_) | DecodedImage::Raw(_) => {
                    if let Some((image_key, _)) = renderer_resources.currently_registered_images.get(&image_hash) {
                        frame.content.push(LayoutRectContent::Image {
                            size: image_size,
                            offset: LogicalPosition::zero(),
                            image_rendering: ImageRendering::Auto,
                            alpha_type: AlphaType::PremultipliedAlpha,
                            image_key: *image_key,
                            background_color: ColorU::WHITE,
                        });
                    }
                },
                DecodedImage::Callback(_) => {
                    if let Some((key, descriptor)) = gl_texture_cache.solved_textures.get(&dom_id).and_then(|textures| textures.get(&rect_idx)) {
                        frame.content.push(LayoutRectContent::Image {
                            size: LogicalSize::new(descriptor.width as f32, descriptor.height as f32),
                            offset: LogicalPosition::zero(),
                            image_rendering: ImageRendering::Auto,
                            alpha_type: AlphaType::Alpha,
                            image_key: *key,
                            background_color: ColorU::WHITE,
                        })
                    }
                },
            }
        },
        IFrame(_) => {
            if let Some(iframe_dom_id) = layout_result.iframe_mapping.iter()
            .find_map(|(node_id, dom_id)| if *node_id == rect_idx { Some(*dom_id) } else { None }) {

                let iframe_pipeline_id = PipelineId(iframe_dom_id.inner.max(core::u32::MAX as usize) as u32, referenced_content.document_id.id);
                let cached_display_list = LayoutResult::get_cached_display_list(
                    referenced_content.document_id,
                    iframe_dom_id, // <- important, otherwise it would recurse infinitely
                    referenced_content.epoch,
                    referenced_content.layout_results,
                    referenced_content.full_window_state,
                    referenced_content.gl_texture_cache,
                    referenced_content.renderer_resources,
                    referenced_content.image_cache,
                );
                let iframe_clip_size = positioned_rect.size;
                frame.children.push(DisplayListMsg::IFrame(
                    iframe_pipeline_id,
                    iframe_clip_size,
                    referenced_content.epoch,
                    Box::new(cached_display_list))
                );
            }
        },
    };

    if layout_result.styled_dom.get_css_property_cache().has_border(&html_node, &rect_idx, &styled_node.state) {
        frame.content.push(LayoutRectContent::Border {
            widths: StyleBorderWidths {
                top: layout_result.styled_dom.get_css_property_cache().get_border_top_width(&html_node, &rect_idx, &styled_node.state).cloned(),
                left: layout_result.styled_dom.get_css_property_cache().get_border_left_width(&html_node, &rect_idx, &styled_node.state).cloned(),
                bottom: layout_result.styled_dom.get_css_property_cache().get_border_bottom_width(&html_node, &rect_idx, &styled_node.state).cloned(),
                right: layout_result.styled_dom.get_css_property_cache().get_border_right_width(&html_node, &rect_idx, &styled_node.state).cloned(),
            },
            colors: StyleBorderColors {
                top: layout_result.styled_dom.get_css_property_cache().get_border_top_color(&html_node, &rect_idx, &styled_node.state).cloned(),
                left: layout_result.styled_dom.get_css_property_cache().get_border_left_color(&html_node, &rect_idx, &styled_node.state).cloned(),
                bottom: layout_result.styled_dom.get_css_property_cache().get_border_bottom_color(&html_node, &rect_idx, &styled_node.state).cloned(),
                right: layout_result.styled_dom.get_css_property_cache().get_border_right_color(&html_node, &rect_idx, &styled_node.state).cloned(),
            },
            styles: StyleBorderStyles {
                top: layout_result.styled_dom.get_css_property_cache().get_border_top_style(&html_node, &rect_idx, &styled_node.state).cloned(),
                left: layout_result.styled_dom.get_css_property_cache().get_border_left_style(&html_node, &rect_idx, &styled_node.state).cloned(),
                bottom: layout_result.styled_dom.get_css_property_cache().get_border_bottom_style(&html_node, &rect_idx, &styled_node.state).cloned(),
                right: layout_result.styled_dom.get_css_property_cache().get_border_right_style(&html_node, &rect_idx, &styled_node.state).cloned(),
            },
        });
    }

    match layout_result.scrollable_nodes.overflowing_nodes.get(&AzNodeId::from_crate_internal(Some(rect_idx))) {
        Some(scroll_node) => {
            Some(DisplayListMsg::ScrollFrame(DisplayListScrollFrame {
                parent_rect: scroll_node.parent_rect,
                content_rect: scroll_node.child_rect,
                scroll_id: scroll_node.parent_external_scroll_id,
                scroll_tag: scroll_node.scroll_tag_id,
                frame,
            }))
        },
        None => Some(DisplayListMsg::Frame(frame)),
    }
}

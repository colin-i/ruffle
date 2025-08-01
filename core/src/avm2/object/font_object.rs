use crate::avm2::object::script_object::ScriptObjectData;
use crate::avm2::object::{Object, TObject};
use crate::avm2::{Activation, ClassObject, Error};
use crate::character::Character;
use crate::font::Font;
use crate::utils::HasPrefixField;
use gc_arena::{Collect, Gc, GcWeak, Mutation};
use std::fmt;

/// A class instance allocator that allocates Font objects.
pub fn font_allocator<'gc>(
    class: ClassObject<'gc>,
    activation: &mut Activation<'_, 'gc>,
) -> Result<Object<'gc>, Error<'gc>> {
    let base = ScriptObjectData::new(class);

    let font = if let Some((movie, id)) = activation
        .context
        .library
        .avm2_class_registry()
        .class_symbol(class.inner_class_definition())
    {
        if let Some(lib) = activation.context.library.library_for_movie(movie) {
            if let Some(Character::Font(font)) = lib.character_by_id(id) {
                Some(font)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(FontObject(Gc::new(activation.gc(), FontObjectData { base, font })).into())
}

#[derive(Clone, Collect, Copy)]
#[collect(no_drop)]
pub struct FontObject<'gc>(pub Gc<'gc, FontObjectData<'gc>>);

#[derive(Clone, Collect, Copy, Debug)]
#[collect(no_drop)]
pub struct FontObjectWeak<'gc>(pub GcWeak<'gc, FontObjectData<'gc>>);

impl<'gc> FontObject<'gc> {
    pub fn for_font(mc: &Mutation<'gc>, class: ClassObject<'gc>, font: Font<'gc>) -> Object<'gc> {
        let base = ScriptObjectData::new(class);
        FontObject(Gc::new(
            mc,
            FontObjectData {
                base,
                font: Some(font),
            },
        ))
        .into()
    }

    pub fn font(self) -> Option<Font<'gc>> {
        self.0.font
    }
}

impl<'gc> TObject<'gc> for FontObject<'gc> {
    fn gc_base(&self) -> Gc<'gc, ScriptObjectData<'gc>> {
        HasPrefixField::as_prefix_gc(self.0)
    }
}

#[derive(Collect, HasPrefixField)]
#[collect(no_drop)]
#[repr(C, align(8))]
pub struct FontObjectData<'gc> {
    /// Base script object
    base: ScriptObjectData<'gc>,

    font: Option<Font<'gc>>,
}

impl fmt::Debug for FontObject<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FontObject")
    }
}

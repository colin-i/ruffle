//! Application Domains

use std::cell::Ref;

use crate::avm2::activation::Activation;
use crate::avm2::bytearray::ByteArrayStorage;
use crate::avm2::class::Class;
use crate::avm2::error::{error, reference_error, Error};
use crate::avm2::object::{ByteArrayObject, TObject};
use crate::avm2::property_map::PropertyMap;
use crate::avm2::script::Script;
use crate::avm2::value::Value;
use crate::avm2::{Avm2, Multiname, QName};
use crate::context::UpdateContext;
use crate::string::AvmString;
use gc_arena::{Collect, GcCell, GcWeakCell, Mutation};
use ruffle_macros::istr;
use ruffle_wstr::WStr;

/// Represents a set of scripts and movies that share traits across different
/// script-global scopes.
#[derive(Copy, Clone, Collect)]
#[collect(no_drop)]
pub struct Domain<'gc>(GcCell<'gc, DomainData<'gc>>);

#[derive(Copy, Clone, Collect)]
#[collect(no_drop)]
pub struct DomainWeak<'gc>(GcWeakCell<'gc, DomainData<'gc>>);

#[derive(Clone, Collect)]
#[collect(no_drop)]
struct DomainData<'gc> {
    /// A list of all exported definitions and the script that exported them.
    defs: PropertyMap<'gc, Script<'gc>>,

    /// A map of all Clasess defined in this domain. Used by ClassObject
    /// to perform early interface resolution.
    classes: PropertyMap<'gc, Class<'gc>>,

    /// The parent domain.
    parent: Option<Domain<'gc>>,

    /// The bytearray used for storing domain memory
    ///
    /// Note: While this property is optional, it is not recommended to set it
    /// to `None`. It is only optional to avoid an order-of-events problem in
    /// player globals setup (we need a global domain to put globals into, but
    /// that domain needs the bytearray global)
    pub domain_memory: Option<ByteArrayObject<'gc>>,

    pub default_domain_memory: Option<ByteArrayObject<'gc>>,

    /// All children of this domain. This is intended exclusively for
    /// use with `debug_ui`
    children: Vec<DomainWeak<'gc>>,
}

const MIN_DOMAIN_MEMORY_LENGTH: usize = 1024;

impl<'gc> Domain<'gc> {
    /// Create a new domain with no parent.
    ///
    /// This is intended exclusively for creating the player globals domain,
    /// and stage domain, which are created before ByteArray is available.
    ///
    /// Note: the global domain will be created without valid domain memory.
    /// You must initialize domain memory later on after the ByteArray class is
    /// instantiated but before user code runs.
    pub fn uninitialized_domain(mc: &Mutation<'gc>, parent: Option<Domain<'gc>>) -> Domain<'gc> {
        let domain = Self(GcCell::new(
            mc,
            DomainData {
                defs: PropertyMap::new(),
                classes: PropertyMap::new(),
                parent,
                domain_memory: None,
                default_domain_memory: None,
                children: Vec::new(),
            },
        ));
        if let Some(parent) = parent {
            parent
                .0
                .write(mc)
                .children
                .push(DomainWeak(GcCell::downgrade(domain.0)));
        }
        domain
    }

    pub fn classes(&self) -> Ref<'_, PropertyMap<'gc, Class<'gc>>> {
        Ref::map(self.0.read(), |r| &r.classes)
    }

    pub fn is_playerglobals_domain(&self, avm2: &Avm2<'gc>) -> bool {
        std::ptr::eq(avm2.playerglobals_domain.0.as_ptr(), self.0.as_ptr())
    }

    pub fn children(&self, mc: &Mutation<'gc>) -> Vec<Domain<'gc>> {
        // Take this opportunity to clean up dead children.
        let mut output = Vec::new();
        self.0.write(mc).children.retain(|child| {
            if let Some(child_cell) = GcWeakCell::upgrade(&child.0, mc) {
                output.push(Domain(child_cell));
                true
            } else {
                false
            }
        });
        output
    }

    /// Create a new domain with a given parent.
    ///
    /// This function must not be called before the player globals have been
    /// fully allocated.
    pub fn movie_domain(activation: &mut Activation<'_, 'gc>, parent: Domain<'gc>) -> Domain<'gc> {
        let this = Self(GcCell::new(
            activation.gc(),
            DomainData {
                defs: PropertyMap::new(),
                classes: PropertyMap::new(),
                parent: Some(parent),
                domain_memory: None,
                default_domain_memory: None,
                children: Vec::new(),
            },
        ));

        this.init_default_domain_memory(activation).unwrap();

        parent
            .0
            .write(activation.gc())
            .children
            .push(DomainWeak(GcCell::downgrade(this.0)));

        this
    }

    /// Get the parent of this domain
    pub fn parent_domain(self) -> Option<Domain<'gc>> {
        self.0.read().parent
    }

    /// Determine if something has been defined within the current domain (including parents)
    pub fn has_definition(self, name: QName<'gc>) -> bool {
        let read = self.0.read();

        if read.defs.contains_key(name) {
            return true;
        }

        if let Some(parent) = read.parent {
            return parent.has_definition(name);
        }

        false
    }

    /// Determine if a class has been defined within the current domain (including parents)
    pub fn has_class(self, name: QName<'gc>) -> bool {
        let read = self.0.read();

        if read.classes.contains_key(name) {
            return true;
        }

        if let Some(parent) = read.parent {
            return parent.has_class(name);
        }

        false
    }

    /// Resolve a Multiname and return the script that provided it.
    ///
    /// If a name does not exist or cannot be resolved, no script or name will
    /// be returned.
    pub fn get_defining_script(
        self,
        multiname: &Multiname<'gc>,
    ) -> Option<(QName<'gc>, Script<'gc>)> {
        let read = self.0.read();

        if let Some(name) = multiname.local_name() {
            if let Some((ns, script)) = read.defs.get_with_ns_for_multiname(multiname) {
                let qname = QName::new(ns, name);
                return Some((qname, *script));
            }
        }

        if let Some(parent) = read.parent {
            return parent.get_defining_script(multiname);
        }

        None
    }

    fn get_class_inner(self, multiname: &Multiname<'gc>) -> Option<Class<'gc>> {
        let read = self.0.read();
        if let Some(class) = read.classes.get_for_multiname(multiname).copied() {
            return Some(class);
        }

        if let Some(parent) = read.parent {
            return parent.get_class_inner(multiname);
        }

        None
    }

    pub fn get_class(
        self,
        context: &mut UpdateContext<'gc>,
        multiname: &Multiname<'gc>,
    ) -> Option<Class<'gc>> {
        let class = self.get_class_inner(multiname);

        if let Some(class) = class {
            if let Some(param) = multiname.param() {
                if let Some(param) = param {
                    if let Some(resolved_param) = self.get_class(context, &param) {
                        return Some(Class::with_type_param(context, class, Some(resolved_param)));
                    }
                    return None;
                } else {
                    return Some(Class::with_type_param(context, class, None));
                }
            }
        }
        class
    }

    /// Resolve a Multiname and return the script that provided it.
    ///
    /// If a name does not exist or cannot be resolved, an error will be thrown.
    pub fn find_defining_script(
        self,
        activation: &mut Activation<'_, 'gc>,
        multiname: &Multiname<'gc>,
    ) -> Result<(QName<'gc>, Script<'gc>), Error<'gc>> {
        match self.get_defining_script(multiname) {
            Some(val) => Ok(val),
            None => Err(Error::avm_error(reference_error(
                activation,
                &format!(
                    "Error #1065: Variable {} is not defined.",
                    multiname.local_name().unwrap_or(istr!("*"))
                ),
                1065,
            )?)),
        }
    }

    /// Retrieve a value from this domain.
    pub fn get_defined_value(
        self,
        activation: &mut Activation<'_, 'gc>,
        name: QName<'gc>,
    ) -> Result<Value<'gc>, Error<'gc>> {
        let (name, script) = self.find_defining_script(activation, &name.into())?;
        let globals = script.globals(activation.context)?;

        Value::from(globals).get_property(&name.into(), activation)
    }

    /// Retrieve a value from this domain, with special handling for 'Vector.<SomeType>'.
    /// This is used by `getQualifiedClassName, ApplicationDomain.getDefinition, and ApplicationDomain.hasDefinition`.
    pub fn get_defined_value_handling_vector(
        self,
        activation: &mut Activation<'_, 'gc>,
        mut name: AvmString<'gc>,
    ) -> Result<Value<'gc>, Error<'gc>> {
        // Special-case lookups of `Vector.<SomeType>` - these get internally converted
        // to a lookup of `Vector,` a lookup of `SomeType`, and `vector_class.apply(some_type_class)`
        let mut type_name = None;
        if (name.starts_with(WStr::from_units(b"__AS3__.vec::Vector.<"))
            || name.starts_with(WStr::from_units(b"Vector.<")))
            && name.ends_with(WStr::from_units(b">"))
        {
            let start = name.find(WStr::from_units(b".<")).unwrap();

            type_name = Some(AvmString::new(
                activation.gc(),
                &name[(start + 2)..(name.len() - 1)],
            ));
            name = AvmString::new_ascii_static(activation.gc(), b"__AS3__.vec::Vector");
        }
        // FIXME - is this the correct api version?
        let api_version = activation.avm2().root_api_version;
        let name = QName::from_qualified_name(name, api_version, activation.context);

        let res = self.get_defined_value(activation, name);

        if let Some(type_name) = type_name {
            let type_class = self.get_defined_value_handling_vector(activation, type_name)?;
            if let Ok(res) = res {
                let class = res.as_object().ok_or_else(|| {
                    Error::rust_error(format!("Vector type {res:?} was not an object").into())
                })?;
                return class.apply(activation, &[type_class]).map(|obj| obj.into());
            }
        }
        res
    }

    pub fn get_defined_names(&self) -> Vec<QName<'gc>> {
        self.0
            .read()
            .defs
            .iter()
            .map(|(name, namespace, _)| QName::new(namespace, name))
            .collect()
    }

    /// Export a definition from a script into the current application domain.
    ///
    /// This does nothing if the definition already exists in this domain or a parent.
    pub fn export_definition(self, name: QName<'gc>, script: Script<'gc>, mc: &Mutation<'gc>) {
        if self.has_definition(name) {
            return;
        }

        self.0.write(mc).defs.insert(name, script);
    }

    /// Export a class into the current application domain.
    ///
    /// This does nothing if the definition already exists in this domain or a parent.
    pub fn export_class(&self, export_name: QName<'gc>, class: Class<'gc>, mc: &Mutation<'gc>) {
        if self.has_class(export_name) {
            return;
        }
        self.0.write(mc).classes.insert(export_name, class);
    }

    pub fn defs(&self) -> Ref<PropertyMap<'gc, Script<'gc>>> {
        Ref::map(self.0.read(), |this| &this.defs)
    }

    pub fn is_default_domain_memory(&self) -> bool {
        let read = self.0.read();
        let domain_memory_ptr = read.domain_memory.expect("Missing domain memory").as_ptr();
        let default_domain_memory_ptr = read
            .default_domain_memory
            .expect("Missing default domain memory")
            .as_ptr();
        std::ptr::eq(domain_memory_ptr, default_domain_memory_ptr)
    }

    pub fn domain_memory(&self) -> ByteArrayObject<'gc> {
        self.0
            .read()
            .domain_memory
            .expect("Domain must have valid memory at all times")
    }

    pub fn set_domain_memory(
        &self,
        activation: &mut Activation<'_, 'gc>,
        domain_memory: Option<ByteArrayObject<'gc>>,
    ) -> Result<(), Error<'gc>> {
        let mut write = self.0.write(activation.gc());
        let memory = if let Some(domain_memory) = domain_memory {
            if domain_memory.storage().len() < MIN_DOMAIN_MEMORY_LENGTH {
                return Err(Error::avm_error(error(
                    activation,
                    "Error #1504: End of file.",
                    1504,
                )?));
            }
            domain_memory
        } else {
            write
                .default_domain_memory
                .expect("Default domain memory not initialized")
        };
        write.domain_memory = Some(memory);
        Ok(())
    }

    /// Allocate the default domain memory for this domain, if it does not
    /// already exist.
    ///
    /// This function is only necessary to be called for domains created via
    /// `global_domain`. It will do nothing on already fully-initialized
    /// domains.
    pub fn init_default_domain_memory(
        self,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<(), Error<'gc>> {
        let initial_data = vec![0; MIN_DOMAIN_MEMORY_LENGTH];
        let storage = ByteArrayStorage::from_vec(initial_data);

        let domain_memory = ByteArrayObject::from_storage(activation, storage)?;

        let mut write = self.0.write(activation.gc());

        assert!(
            write.domain_memory.is_none(),
            "Already initialized domain memory!"
        );
        assert!(
            write.default_domain_memory.is_none(),
            "Already initialized domain memory!"
        );

        write.domain_memory = Some(domain_memory);
        write.default_domain_memory = Some(domain_memory);

        Ok(())
    }

    pub fn as_ptr(self) -> *const DomainPtr {
        self.0.as_ptr() as _
    }
}

pub enum DomainPtr {}

impl PartialEq for Domain<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl Eq for Domain<'_> {}

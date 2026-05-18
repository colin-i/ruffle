use crate::avm1::{Activation, ActivationIdentifier};
use crate::context::UpdateContext;
use crate::debug::debug_message_out::DebugMessageOut;
use crate::debug::debug_provider::DebugProvider;
use crate::debug::display_object_info::DisplayObjectInfo;
use crate::debug::targeted_message::TargetedMsg;
use crate::display_object::TDisplayObjectContainer;
use crate::display_object::MovieClip;
use crate::display_object::{
    TDisplayObject,
    TInteractiveObject,
};
use crate::string::AvmString;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct MovieClipInfo {
    depth: i32,
    current_frame: u16,
    is_focusable: bool,
    enabled: bool,
}

pub struct MovieClipDebugger<'gc> {
    tgt: MovieClip<'gc>,
}

impl<'gc> MovieClipDebugger<'gc> {
    pub fn with(tgt: MovieClip<'gc>) -> Self {
        Self { tgt }
    }
}

impl<'gc> DebugProvider<'gc> for MovieClipDebugger<'gc> {
    fn dispatch(
        &mut self,
        evt: TargetedMsg,
        context: &mut UpdateContext<'gc>,
    ) -> Option<DebugMessageOut> {
        match evt {
            TargetedMsg::Stop => {
                self.tgt.stop(context);
                Some(DebugMessageOut::GenericResult { success: true })
            }
            TargetedMsg::GetInfo => Some(DebugMessageOut::DisplayObjectInfo(
                DisplayObjectInfo::MovieClip(MovieClipInfo {
                    depth: self.tgt.depth(),
                    current_frame: self.tgt.current_frame(),
                    is_focusable: self.tgt.is_focusable(context),
                    enabled: self.tgt.avm1_enabled(context),
                }),
            )),
            TargetedMsg::GetChildren => {
                for x in self.tgt.as_container().unwrap().iter_render_list() {
                    //TODO: msg
                }
                Some(DebugMessageOut::GenericResult { success: true })
            }
            TargetedMsg::GetProps => {
                let mut activation = Activation::from_stub(
                    context,
                    ActivationIdentifier::root("[Debugger]"),
                );

                let obj = self.tgt.object1().unwrap();
                let keys = obj.get_keys(&mut activation, false);

                let mut out_keys: Vec<String> = Vec::new();
                for key in &keys {
                    out_keys.push(key.to_utf8_lossy().to_string());
                }

                Some(DebugMessageOut::GetPropsResult { keys: out_keys })
            }
            TargetedMsg::GetPropValue { name } => {
                let mut activation = Activation::from_stub(
                    context,
                    ActivationIdentifier::root("[Debugger]"),
                );

                let obj = self.tgt.object1().unwrap();
                let val = obj.get(
                    AvmString::new_utf8(activation.context.gc_context, name),
                    &mut activation,
                );
                //TODO: msg

                Some(DebugMessageOut::GenericResult { success: true })
            }
            TargetedMsg::SetPropValue { name, value } => {
                let mut activation = Activation::from_stub(
                    context,
                    ActivationIdentifier::root("[Debugger]"),
                );

                let obj = self.tgt.object1().unwrap();
                obj.set(
                    AvmString::new_utf8(activation.context.gc_context, name),
                    value.as_avm1(&mut activation.context),
                    &mut activation,
                )
                .unwrap();
                Some(DebugMessageOut::GenericResult { success: true })
            }
        }
    }
}

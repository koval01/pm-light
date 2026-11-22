#[cfg(target_os = "macos")]
mod names {
    use super::super::key_of;
    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, NSObject},
    };
    use objc2_foundation::NSString;

    /// An object of Foundation's own that answers to `key`, as a feature does: a sort descriptor.
    fn with_key(key: Option<&str>) -> Retained<AnyObject> {
        let class = AnyClass::get(c"NSSortDescriptor").unwrap();
        let key = key.map(NSString::from_str);
        // Safety: a public class method, with a string or nothing, and a flag.
        unsafe { msg_send![class, sortDescriptorWithKey: key.as_deref(), ascending: true] }
    }

    #[test]
    fn a_feature_is_known_by_its_key() {
        let feature = with_key(Some("PeerConnectionEnabled"));
        assert_eq!(key_of(&feature).map(|key| key.to_string()).as_deref(), Some("PeerConnectionEnabled"));
    }

    #[test]
    fn an_object_with_no_key_is_passed_over_not_sent_it() {
        // Sent `key`, a plain object raises an exception, which ends the program.
        assert_eq!(key_of(&NSObject::new()), None);
    }

    #[test]
    fn a_key_that_is_nothing_is_passed_over() {
        assert_eq!(key_of(&with_key(None)), None);
    }
}

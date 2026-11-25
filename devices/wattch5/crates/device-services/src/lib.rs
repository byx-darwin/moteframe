//! Hardware-independent, allocation-free device business services.
//!
//! The initial service is the connection-local core session. It consumes complete
//! logical requests produced by `device-protocol`; transport, security identity,
//! durable storage and real peripheral backends remain separate integration jobs.
#![no_std]

mod session;
pub use session::{
    AccessSnapshot, AccessState, AnonymousAccess, BoundedSession, BuildProfile, Config,
    ConfigError, DeviceInfo, Reason, ServiceHandler, Session, SessionError, SessionState, Status,
    TrustedAccessProvider,
};

mod properties;
pub use properties::{
    PropertyAccess, PropertyBackend, PropertyConfigError, PropertyDescriptor,
    PropertyDispatchError, PropertyFailure, PropertyHandler, PropertyOperation, PropertyService,
    PropertyValue, RevisionChange,
};

mod time;
pub use time::{SoftwareClock, TimeError, UtcMapping};
mod router;
pub use router::{HandlerTable, HandlerTableError};
mod time_handler;
pub use time_handler::{ClockSource, ClockSourceError, TimeAccess, TimeConfigError, TimeHandler};

mod resources;
pub use resources::{
    CoreRequest, CoreResourceHandler, CoreResources, DatasetIdentity, GrantRequest,
    ResourceConfigError, ResourceFailure,
};

mod typed_router;
pub use typed_router::{HandlerGroup, TypedHandlerTable};

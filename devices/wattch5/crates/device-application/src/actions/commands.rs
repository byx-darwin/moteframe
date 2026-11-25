//! Typed optional action bodies. No backend or authorization is implied by decode.
use super::ActionError;
/// An action selected at compile time; fields use candidate V0.6 units and enum dictionaries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    #[cfg(feature = "system")]
    /// Module02 opcode30.
    Reboot {
        /// Contract field `delay_s`.
        delay_s: u32,
    },
    #[cfg(feature = "system")]
    /// Module02 opcode31.
    Poweroff {
        /// Contract field `delay_s`.
        delay_s: u32,
    },
    #[cfg(feature = "system")]
    /// Module02 opcode32.
    RestoreName,
    #[cfg(feature = "action-find")]
    /// Module06 opcode30.
    Find {
        /// Contract field `target`.
        target: u8,
        /// Contract field `pattern`.
        pattern: u8,
        /// Contract field `duration_s`.
        duration_s: u16,
    },
    #[cfg(feature = "action-find")]
    /// Module06 opcode31.
    StopFind {
        /// Contract field `find_id`.
        find_id: u32,
    },
    #[cfg(feature = "action-find")]
    /// Module06 opcode32.
    FindResult {
        /// Contract field `find_id`.
        find_id: u32,
        /// Contract field `result`.
        result: u8,
    },
    #[cfg(feature = "action-phone")]
    /// Module07 opcode30.
    Heartbeat {
        /// Contract field `token`.
        token: u32,
    },
    #[cfg(feature = "action-call")]
    /// Module08 opcode30.
    CallEnabled {
        /// Contract field `enabled`.
        enabled: u8,
    },
    #[cfg(feature = "action-notification")]
    /// Module30 opcode30.
    CallActionResult {
        /// Contract field `notification`.
        notification: u32,
        /// Contract field `action`.
        action: u8,
        /// Contract field `result`.
        result: u8,
    },
    #[cfg(feature = "action-media")]
    /// Module33 opcode30.
    Camera {
        /// Contract field `enabled`.
        enabled: u8,
    },
    #[cfg(feature = "action-media")]
    /// Module33 opcode31.
    CameraAck {
        /// Contract field `event_id`.
        event_id: u32,
        /// Contract field `result`.
        result: u8,
    },
    #[cfg(feature = "action-media")]
    /// Module33 opcode32.
    Music {
        /// Contract field `action`.
        action: u8,
    },
    #[cfg(feature = "action-media")]
    /// Module33 opcode33.
    ActionAck {
        /// Contract field `event_id`.
        event_id: u32,
        /// Contract field `result`.
        result: u8,
    },
    #[cfg(feature = "action-media")]
    /// Module33 opcode34.
    Gimbal {
        /// Contract field `action_id`.
        action_id: u32,
        /// Contract field `action`.
        action: u8,
        /// Contract field `direction`.
        direction: u8,
        /// Contract field `mode`.
        mode: u8,
        /// Contract field `value`.
        value: u32,
    },
}
impl core::fmt::Debug for Action {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Action")
            .field("route", &self.route())
            .finish_non_exhaustive()
    }
}
impl Action {
    /// Module and opcode.
    #[must_use]
    pub const fn route(self) -> (u8, u8) {
        match self {
            #[cfg(feature = "system")]
            Self::Reboot { .. } => (2, 48),
            #[cfg(feature = "system")]
            Self::Poweroff { .. } => (2, 49),
            #[cfg(feature = "system")]
            Self::RestoreName => (2, 50),
            #[cfg(feature = "action-find")]
            Self::Find { .. } => (6, 48),
            #[cfg(feature = "action-find")]
            Self::StopFind { .. } => (6, 49),
            #[cfg(feature = "action-find")]
            Self::FindResult { .. } => (6, 50),
            #[cfg(feature = "action-phone")]
            Self::Heartbeat { .. } => (7, 48),
            #[cfg(feature = "action-call")]
            Self::CallEnabled { .. } => (8, 48),
            #[cfg(feature = "action-notification")]
            Self::CallActionResult { .. } => (48, 48),
            #[cfg(feature = "action-media")]
            Self::Camera { .. } => (51, 48),
            #[cfg(feature = "action-media")]
            Self::CameraAck { .. } => (51, 49),
            #[cfg(feature = "action-media")]
            Self::Music { .. } => (51, 50),
            #[cfg(feature = "action-media")]
            Self::ActionAck { .. } => (51, 51),
            #[cfg(feature = "action-media")]
            Self::Gimbal { .. } => (51, 52),
        }
    }
    /// Decode exact fields; disabled modules are unsupported.
    /// # Errors
    /// Returns Unsupported, Invalid or Length for unknown routes or invalid bodies.
    pub fn decode(module: u8, opcode: u8, bytes: &[u8]) -> Result<Self, ActionError> {
        let value = match (module, opcode, bytes) {
            #[cfg(feature = "system")]
            (2, 48, [delay_s_0, delay_s_1, delay_s_2, delay_s_3]) => Self::Reboot {
                delay_s: u32::from_le_bytes([*delay_s_0, *delay_s_1, *delay_s_2, *delay_s_3]),
            },
            #[cfg(feature = "system")]
            (2, 49, [delay_s_0, delay_s_1, delay_s_2, delay_s_3]) => Self::Poweroff {
                delay_s: u32::from_le_bytes([*delay_s_0, *delay_s_1, *delay_s_2, *delay_s_3]),
            },
            #[cfg(feature = "system")]
            (2, 50, []) => Self::RestoreName,
            #[cfg(feature = "action-find")]
            (6, 48, [target_0, pattern_0, duration_s_0, duration_s_1]) => Self::Find {
                target: *target_0,
                pattern: *pattern_0,
                duration_s: u16::from_le_bytes([*duration_s_0, *duration_s_1]),
            },
            #[cfg(feature = "action-find")]
            (6, 49, [find_id_0, find_id_1, find_id_2, find_id_3]) => Self::StopFind {
                find_id: u32::from_le_bytes([*find_id_0, *find_id_1, *find_id_2, *find_id_3]),
            },
            #[cfg(feature = "action-find")]
            (6, 50, [find_id_0, find_id_1, find_id_2, find_id_3, result_0]) => Self::FindResult {
                find_id: u32::from_le_bytes([*find_id_0, *find_id_1, *find_id_2, *find_id_3]),
                result: *result_0,
            },
            #[cfg(feature = "action-phone")]
            (7, 48, [token_0, token_1, token_2, token_3]) => Self::Heartbeat {
                token: u32::from_le_bytes([*token_0, *token_1, *token_2, *token_3]),
            },
            #[cfg(feature = "action-call")]
            (8, 48, [enabled_0]) => Self::CallEnabled {
                enabled: *enabled_0,
            },
            #[cfg(feature = "action-notification")]
            (
                48,
                48,
                [
                    notification_0,
                    notification_1,
                    notification_2,
                    notification_3,
                    action_0,
                    result_0,
                ],
            ) => Self::CallActionResult {
                notification: u32::from_le_bytes([
                    *notification_0,
                    *notification_1,
                    *notification_2,
                    *notification_3,
                ]),
                action: *action_0,
                result: *result_0,
            },
            #[cfg(feature = "action-media")]
            (51, _, _) => Self::decode_media(opcode, bytes)?,
            _ => return Err(ActionError::Unsupported),
        };
        value.validate()?;
        Ok(value)
    }
    #[cfg(feature = "action-media")]
    fn decode_media(opcode: u8, bytes: &[u8]) -> Result<Self, ActionError> {
        Ok(match (51, opcode, bytes) {
            #[cfg(feature = "action-media")]
            (51, 48, [enabled_0]) => Self::Camera {
                enabled: *enabled_0,
            },
            #[cfg(feature = "action-media")]
            (51, 49, [event_id_0, event_id_1, event_id_2, event_id_3, result_0]) => {
                Self::CameraAck {
                    event_id: u32::from_le_bytes([
                        *event_id_0,
                        *event_id_1,
                        *event_id_2,
                        *event_id_3,
                    ]),
                    result: *result_0,
                }
            }
            #[cfg(feature = "action-media")]
            (51, 50, [action_0]) => Self::Music { action: *action_0 },
            #[cfg(feature = "action-media")]
            (51, 51, [event_id_0, event_id_1, event_id_2, event_id_3, result_0]) => {
                Self::ActionAck {
                    event_id: u32::from_le_bytes([
                        *event_id_0,
                        *event_id_1,
                        *event_id_2,
                        *event_id_3,
                    ]),
                    result: *result_0,
                }
            }
            #[cfg(feature = "action-media")]
            (
                51,
                52,
                [
                    action_id_0,
                    action_id_1,
                    action_id_2,
                    action_id_3,
                    action_0,
                    direction_0,
                    mode_0,
                    value_0,
                    value_1,
                    value_2,
                    value_3,
                ],
            ) => Self::Gimbal {
                action_id: u32::from_le_bytes([
                    *action_id_0,
                    *action_id_1,
                    *action_id_2,
                    *action_id_3,
                ]),
                action: *action_0,
                direction: *direction_0,
                mode: *mode_0,
                value: u32::from_le_bytes([*value_0, *value_1, *value_2, *value_3]),
            },
            _ => return Err(ActionError::Unsupported),
        })
    }
    /// Encode validated body into caller storage.
    /// # Errors
    /// Returns Invalid or Capacity without executing the action.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, ActionError> {
        self.validate()?;
        let mut body = [0u8; 11];
        let n = match self {
            #[cfg(feature = "system")]
            Self::Reboot { delay_s } => {
                body[0..4].copy_from_slice(&delay_s.to_le_bytes());
                4
            }
            #[cfg(feature = "system")]
            Self::Poweroff { delay_s } => {
                body[0..4].copy_from_slice(&delay_s.to_le_bytes());
                4
            }
            #[cfg(feature = "system")]
            Self::RestoreName => 0,
            #[cfg(feature = "action-find")]
            Self::Find {
                target,
                pattern,
                duration_s,
            } => {
                body[0] = target;
                body[1] = pattern;
                body[2..4].copy_from_slice(&duration_s.to_le_bytes());
                4
            }
            #[cfg(feature = "action-find")]
            Self::StopFind { find_id } => {
                body[0..4].copy_from_slice(&find_id.to_le_bytes());
                4
            }
            #[cfg(feature = "action-find")]
            Self::FindResult { find_id, result } => {
                body[0..4].copy_from_slice(&find_id.to_le_bytes());
                body[4] = result;
                5
            }
            #[cfg(feature = "action-phone")]
            Self::Heartbeat { token } => {
                body[0..4].copy_from_slice(&token.to_le_bytes());
                4
            }
            #[cfg(feature = "action-call")]
            Self::CallEnabled { enabled } => {
                body[0] = enabled;
                1
            }
            #[cfg(feature = "action-notification")]
            Self::CallActionResult {
                notification,
                action,
                result,
            } => {
                body[0..4].copy_from_slice(&notification.to_le_bytes());
                body[4] = action;
                body[5] = result;
                6
            }
            #[cfg(feature = "action-media")]
            Self::Camera { enabled } => {
                body[0] = enabled;
                1
            }
            #[cfg(feature = "action-media")]
            Self::CameraAck { event_id, result } => {
                body[0..4].copy_from_slice(&event_id.to_le_bytes());
                body[4] = result;
                5
            }
            #[cfg(feature = "action-media")]
            Self::Music { action } => {
                body[0] = action;
                1
            }
            #[cfg(feature = "action-media")]
            Self::ActionAck { event_id, result } => {
                body[0..4].copy_from_slice(&event_id.to_le_bytes());
                body[4] = result;
                5
            }
            #[cfg(feature = "action-media")]
            Self::Gimbal {
                action_id,
                action,
                direction,
                mode,
                value,
            } => {
                body[0..4].copy_from_slice(&action_id.to_le_bytes());
                body[4] = action;
                body[5] = direction;
                body[6] = mode;
                body[7..11].copy_from_slice(&value.to_le_bytes());
                11
            }
        };
        out.get_mut(..n)
            .ok_or(ActionError::Capacity)?
            .copy_from_slice(&body[..n]);
        Ok(n)
    }
    fn validate(self) -> Result<(), ActionError> {
        let valid = match self {
            #[cfg(feature = "system")]
            Self::Reboot { delay_s } => (1..=3600).contains(&delay_s),
            #[cfg(feature = "system")]
            Self::Poweroff { delay_s } => (1..=3600).contains(&delay_s),
            #[cfg(feature = "system")]
            Self::RestoreName => true,
            #[cfg(feature = "action-find")]
            Self::Find {
                target,
                pattern,
                duration_s,
            } => target <= 1 && (1..=3).contains(&pattern) && duration_s != 0,
            #[cfg(feature = "action-find")]
            Self::StopFind { find_id } => find_id != 0,
            #[cfg(feature = "action-find")]
            Self::FindResult { find_id, result } => find_id != 0 && (1..=3).contains(&result),
            #[cfg(feature = "action-phone")]
            Self::Heartbeat { .. } => true,
            #[cfg(feature = "action-call")]
            Self::CallEnabled { enabled } => enabled <= 1,
            #[cfg(feature = "action-notification")]
            Self::CallActionResult {
                notification,
                action,
                result,
            } => notification != 0 && (1..=3).contains(&action) && (1..=5).contains(&result),
            #[cfg(feature = "action-media")]
            Self::Camera { enabled } => enabled <= 1,
            #[cfg(feature = "action-media")]
            Self::CameraAck { event_id, result } => event_id != 0 && result <= 3,
            #[cfg(feature = "action-media")]
            Self::Music { action } => (1..=5).contains(&action),
            #[cfg(feature = "action-media")]
            Self::ActionAck { event_id, result } => event_id != 0 && result <= 3,
            #[cfg(feature = "action-media")]
            Self::Gimbal {
                action_id,
                action,
                direction,
                mode,
                value,
            } => {
                action_id != 0
                    && action <= 1
                    && direction <= 1
                    && mode <= 1
                    && (action == 1 || (direction == 0 && mode == 0 && value == 0))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(ActionError::Invalid)
        }
    }
}

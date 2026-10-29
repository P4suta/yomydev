use serde_json::{Value, json};
use yomydev_core::{Basis, Idle, Presence};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) basis: Basis,
    pub(crate) idle: Idle,
}

impl Verdict {
    pub(crate) fn to_json(self) -> Value {
        let idle_seconds = match self.idle {
            Idle::Since(seconds) => json!(seconds.get()),
            Idle::Unknown => Value::Null,
        };
        json!({
            "presence": self.basis.presence().name(),
            "basis": self.basis.name(),
            "idle_seconds": idle_seconds,
        })
    }
}

pub(crate) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "presence": {"type": "string", "enum": Presence::ALL.map(Presence::name)},
            "basis": {"type": "string", "enum": Basis::ALL.map(Basis::name)},
            "idle_seconds": {"type": ["integer", "null"], "minimum": 0},
        },
        "required": ["presence", "basis", "idle_seconds"],
        "additionalProperties": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use yomydev_core::{CameraFault, IdleSeconds};

    #[test]
    fn renders_presence_basis_and_idle_seconds() {
        let verdict = Verdict {
            basis: Basis::NobodySeen,
            idle: Idle::Since(IdleSeconds::new(412)),
        };
        assert_eq!(
            verdict.to_json(),
            json!({"presence": "absent", "basis": "nobody_seen", "idle_seconds": 412})
        );
    }

    #[test]
    fn unknown_idle_renders_as_null() {
        let verdict = Verdict {
            basis: Basis::Camera(CameraFault::NotAuthorized),
            idle: Idle::Unknown,
        };
        assert_eq!(
            verdict.to_json(),
            json!({"presence": "unknown", "basis": "camera_not_authorized", "idle_seconds": null})
        );
    }

    #[test]
    fn the_schema_lists_every_basis() {
        let names = schema()["properties"]["basis"]["enum"].clone();
        assert_eq!(names.as_array().map(Vec::len), Some(Basis::ALL.len()));
    }
}

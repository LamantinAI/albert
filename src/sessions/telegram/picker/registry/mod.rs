//! Bounded transient controls, not a Session catalog or conversation store.
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use octo_connector_telegram::{CallbackFrame, NativeFrame};
use octo_history::SessionId;
use rand::random;

use super::PickerSettings;
use crate::sessions::{
    error::{SessionError, SessionResult},
    SessionContext,
};

pub(super) const EXPIRED: &str =
    "This list has expired or is unavailable. Use /sessions for a fresh list.";
#[derive(Clone, Debug, Default)]
pub(in crate::sessions::telegram) struct Cursor {
    pub before: Option<i64>,
    pub through: Option<i64>,
    pub previous: Option<String>,
}
#[derive(Clone, Debug)]
pub(super) enum Action {
    Open(SessionId),
    Page(Cursor),
}
#[derive(Clone, Debug)]
pub(in crate::sessions::telegram) struct Binding {
    pub user: String,
    connector: String,
    bot: u64,
    chat: i64,
    actor: u64,
}
impl Binding {
    pub fn native(frame: &NativeFrame, context: &SessionContext) -> SessionResult<Self> {
        let actor = u64::try_from(frame.actor_id.ok_or(SessionError::Denied)?)
            .map_err(|_| SessionError::Denied)?;
        if frame.chat_type != "private"
            || frame.chat_id <= 0
            || actor != frame.chat_id as u64
            || frame.connector.as_str() != context.reply_route().connector()
            || frame.chat_id.to_string() != context.reply_route().channel()
        {
            return Err(SessionError::Denied);
        }
        Ok(Self {
            user: context.private_user_id()?.as_str().into(),
            connector: frame.connector.to_string(),
            bot: frame.bot_id,
            chat: frame.chat_id,
            actor,
        })
    }
    pub fn matches(&self, frame: &CallbackFrame) -> bool {
        frame.connector.as_str() == self.connector
            && frame.bot_id == self.bot
            && frame.actor_id == self.actor
            && frame.message.as_ref().is_some_and(|message| {
                message.chat_type == "private"
                    && message.chat_id == self.chat
                    && message.accessible
                    && message.sender_id == Some(frame.bot_id)
            })
    }
}
#[derive(Clone)]
pub(super) struct Ticket {
    token: String,
    query: String,
    pub binding: Binding,
    pub action: Action,
}
pub(super) enum Reserved {
    New(Ticket),
    Replay(String),
}
enum Receipt {
    Pending,
    Complete(String),
}
struct List {
    binding: Binding,
    cursor: Cursor,
    actions: Vec<Action>,
    expires: Instant,
    receipts: HashMap<String, Receipt>,
}
pub(super) struct Registry {
    settings: PickerSettings,
    lists: HashMap<String, List>,
}
impl Registry {
    pub fn new(settings: PickerSettings) -> Self {
        Self {
            settings,
            lists: HashMap::new(),
        }
    }
    fn clean(&mut self, now: Instant) {
        self.lists.retain(|_, list| list.expires > now);
    }
    pub fn token() -> String {
        format!("{:032x}", random::<u128>())
    }
    pub fn previous(&mut self, cursor: &Cursor, binding: &Binding, now: Instant) -> Option<Cursor> {
        self.clean(now);
        self.lists
            .get(cursor.previous.as_ref()?)
            .filter(|list| {
                list.binding.user == binding.user
                    && list.binding.connector == binding.connector
                    && list.binding.bot == binding.bot
                    && list.binding.chat == binding.chat
            })
            .map(|list| list.cursor.clone())
    }
    pub fn insert(
        &mut self,
        token: String,
        binding: Binding,
        cursor: Cursor,
        actions: Vec<Action>,
        now: Instant,
    ) {
        self.clean(now);
        if self.lists.len() >= self.settings.max_lists {
            if let Some(oldest) = self
                .lists
                .iter()
                .min_by_key(|(_, list)| list.expires)
                .map(|(key, _)| key.clone())
            {
                self.lists.remove(&oldest);
            }
        }
        self.lists.insert(
            token,
            List {
                binding,
                cursor,
                actions,
                expires: now + Duration::from_secs(self.settings.ttl_secs),
                receipts: HashMap::new(),
            },
        );
    }
    pub fn reserve(
        &mut self,
        frame: &CallbackFrame,
        now: Instant,
    ) -> Result<Reserved, &'static str> {
        self.clean(now);
        if frame.query_id.is_empty() || frame.query_id.len() > 256 {
            return Err(EXPIRED);
        }
        let (token, index) = frame
            .data
            .strip_prefix("sp:")
            .and_then(|data| data.split_once(':'))
            .ok_or(EXPIRED)?;
        let index = index.parse::<usize>().map_err(|_| EXPIRED)?;
        let list = self
            .lists
            .get(token)
            .filter(|list| list.binding.matches(frame))
            .ok_or(EXPIRED)?;
        if index >= list.actions.len() {
            return Err(EXPIRED);
        }
        // Query IDs are bot-scoped and immutable, including across lists.
        for list in self.lists.values().filter(|list| {
            list.binding.connector == frame.connector.as_str() && list.binding.bot == frame.bot_id
        }) {
            if let Some(receipt) = list.receipts.get(&frame.query_id) {
                return Ok(Reserved::Replay(match receipt {Receipt::Pending=>"This click was already accepted and was not repeated. Use /sessions to check the current view.".into(),Receipt::Complete(ack)=>ack.clone()}));
            }
        }
        if list.receipts.len() >= self.settings.max_receipts_per_list {
            self.lists.remove(token);
            return Err(EXPIRED);
        }
        let list = self.lists.get_mut(token).expect("list checked");
        list.receipts
            .insert(frame.query_id.clone(), Receipt::Pending);
        Ok(Reserved::New(Ticket {
            token: token.into(),
            query: frame.query_id.clone(),
            binding: list.binding.clone(),
            action: list.actions[index].clone(),
        }))
    }
    pub fn pending(&mut self, ticket: &Ticket, now: Instant) -> bool {
        self.clean(now);
        self.lists
            .get(&ticket.token)
            .is_some_and(|list| matches!(list.receipts.get(&ticket.query), Some(Receipt::Pending)))
    }
    pub fn finish(&mut self, ticket: &Ticket, ack: &str) {
        if let Some(list) = self.lists.get_mut(&ticket.token) {
            if matches!(list.receipts.get(&ticket.query), Some(Receipt::Pending)) {
                list.receipts
                    .insert(ticket.query.clone(), Receipt::Complete(ack.into()));
            }
        }
    }
}

#[cfg(test)]
impl Registry {
    pub(super) fn has_pending(&self, query: &str) -> bool {
        self.lists
            .values()
            .any(|list| matches!(list.receipts.get(query), Some(Receipt::Pending)))
    }
}
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use octo_connector_telegram::{CallbackFrame, CallbackMessage};
    use octo_core::ConnectorId;
    use octo_history::SessionId;

    use super::{Action, Binding, Cursor, Registry, Reserved, Ticket};
    use crate::sessions::telegram::picker::PickerSettings;

    fn binding() -> Binding {
        Binding {
            user: "owner".into(),
            connector: "telegram".into(),
            bot: 9,
            chat: 7,
            actor: 7,
        }
    }
    fn action() -> Action {
        Action::Open(SessionId::parse("00000000-0000-0000-0000-000000000001").unwrap())
    }
    fn frame(token: &str, query: &str) -> CallbackFrame {
        CallbackFrame {
            connector: ConnectorId::new("telegram"),
            bot_id: 9,
            update_id: 1,
            query_id: query.into(),
            actor_id: 7,
            message: Some(CallbackMessage {
                chat_id: 7,
                message_id: 17,
                chat_type: "private".into(),
                accessible: true,
                sender_id: Some(9),
            }),
            inline_message_id: None,
            data: format!("sp:{token}:0"),
        }
    }
    fn ticket(reserved: Reserved) -> Ticket {
        match reserved {
            Reserved::New(ticket) => ticket,
            _ => panic!("expected new reservation"),
        }
    }
    #[test]
    fn pending_survives_cancellation_and_same_query_never_becomes_new_on_another_live_list() {
        let mut registry = Registry::new(PickerSettings::default());
        let now = Instant::now();
        registry.insert(
            "a".into(),
            binding(),
            Cursor::default(),
            vec![action()],
            now,
        );
        let reserved = ticket(registry.reserve(&frame("a", "q1"), now).unwrap());
        drop(reserved); // A cancelled callback leaves its pre-await reservation.
        assert!(matches!(
            registry.reserve(&frame("a", "q1"), now).unwrap(),
            Reserved::Replay(_)
        ));
        registry.insert(
            "b".into(),
            binding(),
            Cursor::default(),
            vec![action()],
            now,
        );
        assert!(matches!(
            registry.reserve(&frame("b", "q1"), now).unwrap(),
            Reserved::Replay(_)
        ));
        let reserved = ticket(registry.reserve(&frame("b", "q2"), now).unwrap());
        registry.finish(&reserved, "Opened");
        match registry.reserve(&frame("b", "q2"), now).unwrap() {
            Reserved::Replay(ack) => assert_eq!(ack, "Opened"),
            _ => panic!("duplicate was admitted"),
        }
    }
    #[test]
    fn expiry_restart_and_capacity_invalidate_whole_tokens_including_receipts() {
        let settings = PickerSettings {
            ttl_secs: 1,
            max_lists: 1,
            max_receipts_per_list: 1,
            ..PickerSettings::default()
        };
        let mut registry = Registry::new(settings.clone());
        let now = Instant::now();
        registry.insert(
            "a".into(),
            binding(),
            Cursor::default(),
            vec![action()],
            now,
        );
        let reserved = ticket(registry.reserve(&frame("a", "q1"), now).unwrap());
        registry.finish(&reserved, "Opened");
        assert!(matches!(
            registry.reserve(&frame("a", "q1"), now),
            Ok(Reserved::Replay(_))
        ));
        assert!(registry.reserve(&frame("a", "q2"), now).is_err());
        assert!(registry.reserve(&frame("a", "q1"), now).is_err());
        registry.insert(
            "b".into(),
            binding(),
            Cursor::default(),
            vec![action()],
            now,
        );
        let pending = ticket(registry.reserve(&frame("b", "q3"), now).unwrap());
        registry.insert(
            "c".into(),
            binding(),
            Cursor::default(),
            vec![action()],
            now,
        );
        assert!(!registry.pending(&pending, now));
        assert!(registry.reserve(&frame("b", "q3"), now).is_err());
        assert!(registry
            .reserve(&frame("c", "q4"), now + Duration::from_secs(1))
            .is_err());
        assert!(Registry::new(settings)
            .reserve(&frame("c", "q5"), now)
            .is_err());
    }
    #[test]
    fn foreign_transport_facts_are_denied_without_consuming_a_query_receipt() {
        let mut registry = Registry::new(PickerSettings::default());
        let now = Instant::now();
        registry.insert(
            "a".into(),
            binding(),
            Cursor::default(),
            vec![action()],
            now,
        );
        let base = frame("a", "q1");
        let mut bad = Vec::new();
        let mut item = base.clone();
        item.actor_id = 8;
        bad.push(item);
        let mut item = base.clone();
        item.bot_id = 10;
        bad.push(item);
        let mut item = base.clone();
        item.connector = ConnectorId::new("other");
        bad.push(item);
        let mut item = base.clone();
        item.message.as_mut().unwrap().chat_id = 8;
        bad.push(item);
        let mut item = base.clone();
        item.message.as_mut().unwrap().chat_type = "group".into();
        bad.push(item);
        let mut item = base.clone();
        item.message.as_mut().unwrap().accessible = false;
        bad.push(item);
        let mut item = base.clone();
        item.message.as_mut().unwrap().sender_id = Some(10);
        bad.push(item);
        for frame in bad {
            assert!(registry.reserve(&frame, now).is_err());
        }
        assert!(registry.lists["a"].receipts.is_empty());
        assert!(matches!(registry.reserve(&base, now), Ok(Reserved::New(_))));
    }
}

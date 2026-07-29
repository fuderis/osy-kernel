use anylm::api::Message;

/// Counts the tokens count
pub fn count_tokens(msgs: &Vec<Message>) -> usize {
    msgs.iter().map(|m| m.tokens_count).sum()
}

/// Linear context grouping [User -> Assistant + Tool's]
pub fn split_messages(messages: Vec<Message>) -> Vec<Vec<Message>> {
    let mut grouped_turns = Vec::new();
    let mut current_turn = Vec::new();

    for msg in messages {
        if msg.role.is_system() {
            continue;
        }
        if msg.role.is_user() && !current_turn.is_empty() {
            grouped_turns.push(current_turn);
            current_turn = Vec::new();
        }
        current_turn.push(msg);
    }
    if !current_turn.is_empty() {
        grouped_turns.push(current_turn);
    }

    grouped_turns
}

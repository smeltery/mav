use super::*;

pub(super) struct ConsoleQueryBarCompletionProvider(WeakEntity<Console>);

impl CompletionProvider for ConsoleQueryBarCompletionProvider {
    fn completions(
        &self,
        buffer: &Entity<Buffer>,
        buffer_position: language::Anchor,
        _trigger: editor::CompletionContext,
        _window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Task<Result<Vec<CompletionResponse>>> {
        let Some(console) = self.0.upgrade() else {
            return Task::ready(Ok(Vec::new()));
        };

        let support_completions = console
            .read(cx)
            .session
            .read(cx)
            .capabilities()
            .supports_completions_request
            .unwrap_or_default();

        if support_completions {
            self.client_completions(&console, buffer, buffer_position, cx)
        } else {
            self.variable_list_completions(&console, buffer, buffer_position, cx)
        }
    }

    fn is_completion_trigger(
        &self,
        buffer: &Entity<Buffer>,
        position: language::Anchor,
        text: &str,
        trigger_in_words: bool,
        cx: &mut Context<Editor>,
    ) -> bool {
        let mut chars = text.chars();
        let char = if let Some(char) = chars.next() {
            char
        } else {
            return false;
        };

        let snapshot = buffer.read(cx).snapshot();

        let classifier = snapshot
            .char_classifier_at(position)
            .scope_context(Some(CharScopeContext::Completion));
        if trigger_in_words && classifier.is_word(char) {
            return true;
        }

        self.0
            .read_with(cx, |console, cx| {
                console
                    .session
                    .read(cx)
                    .capabilities()
                    .completion_trigger_characters
                    .as_ref()
                    .map(|triggers| triggers.contains(&text.to_string()))
            })
            .ok()
            .flatten()
            .unwrap_or(true)
    }
}

impl ConsoleQueryBarCompletionProvider {
    fn variable_list_completions(
        &self,
        console: &Entity<Console>,
        buffer: &Entity<Buffer>,
        buffer_position: language::Anchor,
        cx: &mut Context<Editor>,
    ) -> Task<Result<Vec<CompletionResponse>>> {
        let (variables, string_matches) = console.update(cx, |console, cx| {
            let mut variables = HashMap::default();
            let mut string_matches = Vec::default();

            for variable in console.variable_list.update(cx, |variable_list, cx| {
                variable_list.completion_variables(cx)
            }) {
                if let Some(evaluate_name) = &variable.evaluate_name
                    && variables
                        .insert(evaluate_name.clone(), variable.value.clone())
                        .is_none()
                {
                    string_matches.push(StringMatchCandidate {
                        id: 0,
                        string: evaluate_name.clone(),
                        char_bag: evaluate_name.chars().collect(),
                    });
                }

                if variables
                    .insert(variable.name.clone(), variable.value.clone())
                    .is_none()
                {
                    string_matches.push(StringMatchCandidate {
                        id: 0,
                        string: variable.name.clone(),
                        char_bag: variable.name.chars().collect(),
                    });
                }
            }

            (variables, string_matches)
        });

        let snapshot = buffer.read(cx).text_snapshot();
        let buffer_text = snapshot.text();

        cx.spawn(async move |_, cx| {
            const LIMIT: usize = 10;
            let matches = fuzzy::match_strings(
                &string_matches,
                &buffer_text,
                true,
                true,
                LIMIT,
                &Default::default(),
                cx.background_executor().clone(),
            )
            .await;

            let completions = matches
                .iter()
                .filter_map(|string_match| {
                    let variable_value = variables.get(&string_match.string)?;

                    Some(project::Completion {
                        replace_range: Self::replace_range_for_completion(
                            &buffer_text,
                            buffer_position,
                            string_match.string.as_bytes(),
                            &snapshot,
                        ),
                        new_text: string_match.string.clone(),
                        label: CodeLabel::plain(string_match.string.clone(), None),
                        match_start: None,
                        snippet_deduplication_key: None,
                        icon_path: None,
                        icon_color: None,
                        documentation: Some(CompletionDocumentation::MultiLineMarkdown(
                            variable_value.into(),
                        )),
                        confirm: None,
                        source: project::CompletionSource::Custom,
                        insert_text_mode: None,
                        group: None,
                    })
                })
                .collect::<Vec<_>>();

            Ok(vec![project::CompletionResponse {
                is_incomplete: completions.len() >= LIMIT,
                display_options: CompletionDisplayOptions::default(),
                completions,
            }])
        })
    }

    pub(super) fn replace_range_for_completion(
        buffer_text: &String,
        buffer_position: Anchor,
        new_bytes: &[u8],
        snapshot: &TextBufferSnapshot,
    ) -> Range<Anchor> {
        let buffer_offset = buffer_position.to_offset(snapshot);
        let buffer_bytes = &buffer_text.as_bytes()[0..buffer_offset];

        let mut prefix_len = 0;
        for i in (0..new_bytes.len()).rev() {
            if buffer_bytes.ends_with(&new_bytes[0..i]) {
                prefix_len = i;
                break;
            }
        }

        let start = snapshot.clip_offset(buffer_offset - prefix_len, Bias::Left);

        snapshot.anchor_before(start)..buffer_position
    }

    const fn completion_type_score(completion_type: CompletionItemType) -> usize {
        match completion_type {
            CompletionItemType::Field | CompletionItemType::Property => 0,
            CompletionItemType::Variable | CompletionItemType::Value => 1,
            CompletionItemType::Method
            | CompletionItemType::Function
            | CompletionItemType::Constructor => 2,
            CompletionItemType::Class
            | CompletionItemType::Interface
            | CompletionItemType::Module => 3,
            _ => 4,
        }
    }

    fn completion_item_sort_text(completion_item: &CompletionItem) -> String {
        completion_item.sort_text.clone().unwrap_or_else(|| {
            format!(
                "{:03}_{}",
                Self::completion_type_score(
                    completion_item.type_.unwrap_or(CompletionItemType::Text)
                ),
                completion_item.label.to_ascii_lowercase()
            )
        })
    }

    fn client_completions(
        &self,
        console: &Entity<Console>,
        buffer: &Entity<Buffer>,
        buffer_position: language::Anchor,
        cx: &mut Context<Editor>,
    ) -> Task<Result<Vec<CompletionResponse>>> {
        let completion_task = console.update(cx, |console, cx| {
            console.session.update(cx, |state, cx| {
                let frame_id = console.stack_frame_list.read(cx).opened_stack_frame_id();

                state.completions(
                    CompletionsQuery::new(buffer.read(cx), buffer_position, frame_id),
                    cx,
                )
            })
        });
        let snapshot = buffer.read(cx).text_snapshot();
        cx.background_executor().spawn(async move {
            let completions = completion_task.await?;

            let buffer_text = snapshot.text();

            let completions = completions
                .into_iter()
                .map(|completion| {
                    let sort_text = Self::completion_item_sort_text(&completion);
                    let new_text = completion
                        .text
                        .as_ref()
                        .unwrap_or(&completion.label)
                        .to_owned();

                    project::Completion {
                        replace_range: Self::replace_range_for_completion(
                            &buffer_text,
                            buffer_position,
                            new_text.as_bytes(),
                            &snapshot,
                        ),
                        new_text,
                        label: CodeLabel::plain(completion.label, None),
                        icon_path: None,
                        icon_color: None,
                        documentation: completion.detail.map(|detail| {
                            CompletionDocumentation::MultiLineMarkdown(detail.into())
                        }),
                        match_start: None,
                        snippet_deduplication_key: None,
                        confirm: None,
                        source: project::CompletionSource::Dap { sort_text },
                        insert_text_mode: None,
                        group: None,
                    }
                })
                .collect();

            Ok(vec![project::CompletionResponse {
                completions,
                display_options: CompletionDisplayOptions::default(),
                is_incomplete: false,
            }])
        })
    }
}

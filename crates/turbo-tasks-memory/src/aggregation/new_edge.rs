use super::{
    balance_queue::BalanceQueue,
    in_progress::start_in_progress_all,
    increase::{
        increase_aggregation_number_immediately, PreparedInternalIncreaseAggregationNumber,
        LEAF_NUMBER,
    },
    increase_aggregation_number_internal, notify_new_follower,
    notify_new_follower::PreparedNotifyNewFollower,
    optimize::optimize_aggregation_number_for_followers,
    AggregationContext, AggregationNode, PreparedInternalOperation, PreparedOperation, StackVec,
};

#[cfg(test)]
const BUFFER_SPACE: u32 = 1;
#[cfg(not(test))]
const BUFFER_SPACE: u32 = 2;

const MAX_UPPERS_TIMES_CHILDREN: usize = 32;

const MAX_AFFECTED_NODES: usize = 4096;

#[tracing::instrument(level = tracing::Level::TRACE, name = "handle_new_edge_preparation", skip_all)]
pub fn handle_new_edge<'l, C: AggregationContext>(
    ctx: &C,
    origin: &mut C::Guard<'l>,
    origin_id: &C::NodeRef,
    target_id: &C::NodeRef,
    number_of_children: usize,
) -> impl PreparedOperation<C> {
    match **origin {
        AggregationNode::Leaf {
            ref mut aggregation_number,
            ref uppers,
        } => {
            if number_of_children.count_ones() == 1
                && (uppers.len() + 1) * number_of_children >= MAX_UPPERS_TIMES_CHILDREN
            {
                let uppers = uppers.iter().cloned().collect::<StackVec<_>>();
                start_in_progress_all(ctx, &uppers);
                let increase = increase_aggregation_number_immediately(
                    ctx,
                    origin,
                    origin_id.clone(),
                    LEAF_NUMBER,
                    LEAF_NUMBER,
                )
                .unwrap();
                Some(PreparedNewEdge::Upgraded {
                    uppers,
                    target_id: target_id.clone(),
                    increase,
                })
            } else {
                let min_aggregation_number = *aggregation_number as u32 + 1;
                let target_aggregation_number = *aggregation_number as u32 + 1 + BUFFER_SPACE;
                let uppers = uppers.iter().cloned().collect::<StackVec<_>>();
                start_in_progress_all(ctx, &uppers);
                Some(PreparedNewEdge::Leaf {
                    min_aggregation_number,
                    target_aggregation_number,
                    uppers,
                    target_id: target_id.clone(),
                })
            }
        }
        AggregationNode::Aggegating(_) => origin
            .notify_new_follower_not_in_progress(ctx, origin_id, target_id)
            .map(|notify| PreparedNewEdge::Aggegating {
                target_id: target_id.clone(),
                notify,
            }),
    }
}
enum PreparedNewEdge<C: AggregationContext> {
    Leaf {
        min_aggregation_number: u32,
        target_aggregation_number: u32,
        uppers: StackVec<C::NodeRef>,
        target_id: C::NodeRef,
    },
    Upgraded {
        uppers: StackVec<C::NodeRef>,
        target_id: C::NodeRef,
        increase: PreparedInternalIncreaseAggregationNumber<C>,
    },
    Aggegating {
        notify: PreparedNotifyNewFollower<C>,
        target_id: C::NodeRef,
    },
}

impl<C: AggregationContext> PreparedOperation<C> for PreparedNewEdge<C> {
    type Result = ();
    #[tracing::instrument(level = tracing::Level::TRACE, name = "handle_new_edge", skip_all)]
    fn apply(self, ctx: &C) {
        let mut balance_queue = BalanceQueue::new();
        match self {
            PreparedNewEdge::Leaf {
                min_aggregation_number,
                target_aggregation_number,
                uppers,
                target_id,
            } => {
                let _span = tracing::trace_span!("leaf").entered();
                {
                    let _span =
                        tracing::trace_span!("increase_aggregation_number_internal").entered();
                    // TODO add to prepared
                    increase_aggregation_number_internal(
                        ctx,
                        &mut balance_queue,
                        ctx.node(&target_id),
                        &target_id,
                        min_aggregation_number,
                        target_aggregation_number,
                    );
                }
                let mut affected_nodes = 0;
                for upper_id in uppers {
                    affected_nodes += notify_new_follower(
                        ctx,
                        &mut balance_queue,
                        ctx.node(&upper_id),
                        &upper_id,
                        &target_id,
                        false,
                    );
                    if affected_nodes > MAX_AFFECTED_NODES {
                        handle_expensive_node(ctx, &mut balance_queue, &target_id);
                    }
                }
            }
            PreparedNewEdge::Upgraded {
                uppers,
                target_id,
                increase,
            } => {
                // Since it was added to a leaf node, we would add it to the uppers
                for upper_id in uppers {
                    notify_new_follower(
                        ctx,
                        &mut balance_queue,
                        ctx.node(&upper_id),
                        &upper_id,
                        &target_id,
                        false,
                    );
                }
                // The balancing will attach it to the aggregated node later
                increase.apply(ctx, &mut balance_queue);
            }
            PreparedNewEdge::Aggegating { target_id, notify } => {
                let affected_nodes = notify.apply(ctx, &mut balance_queue);
                if affected_nodes > MAX_AFFECTED_NODES {
                    handle_expensive_node(ctx, &mut balance_queue, &target_id);
                }
            }
        }
        let _span = tracing::trace_span!("balance_queue").entered();
        balance_queue.process(ctx);
    }
}

fn handle_expensive_node<C: AggregationContext>(
    ctx: &C,
    balance_queue: &mut BalanceQueue<C::NodeRef>,
    node_id: &C::NodeRef,
) {
    let node = ctx.node(node_id);
    let Some(followers) = node
        .followers()
        .map(|f| f.iter().cloned().collect::<StackVec<_>>())
    else {
        return;
    };
    drop(node);
    optimize_aggregation_number_for_followers(ctx, balance_queue, node_id, followers, true);
}

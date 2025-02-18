use crate::statistics::event::Event;
use crate::statistics::repository::Repository;

pub async fn handle_event(event: Event, stats_repository: &Repository) {
    match event {
        // TCP4
        Event::Tcp4Announce => {
            stats_repository.increase_tcp4_announces().await;
            stats_repository.increase_tcp4_connections().await;
        }
        Event::Tcp4Scrape => {
            stats_repository.increase_tcp4_scrapes().await;
            stats_repository.increase_tcp4_connections().await;
        }

        // TCP6
        Event::Tcp6Announce => {
            stats_repository.increase_tcp6_announces().await;
            stats_repository.increase_tcp6_connections().await;
        }
        Event::Tcp6Scrape => {
            stats_repository.increase_tcp6_scrapes().await;
            stats_repository.increase_tcp6_connections().await;
        }
    }

    tracing::debug!("stats: {:?}", stats_repository.get_stats().await);
}

#[cfg(test)]
mod tests {
    use crate::statistics::event::handler::handle_event;
    use crate::statistics::event::Event;
    use crate::statistics::repository::Repository;

    #[tokio::test]
    async fn should_increase_the_tcp4_announces_counter_when_it_receives_a_tcp4_announce_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp4Announce, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp4_announces_handled, 1);
    }

    #[tokio::test]
    async fn should_increase_the_tcp4_connections_counter_when_it_receives_a_tcp4_announce_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp4Announce, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp4_connections_handled, 1);
    }

    #[tokio::test]
    async fn should_increase_the_tcp4_scrapes_counter_when_it_receives_a_tcp4_scrape_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp4Scrape, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp4_scrapes_handled, 1);
    }

    #[tokio::test]
    async fn should_increase_the_tcp4_connections_counter_when_it_receives_a_tcp4_scrape_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp4Scrape, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp4_connections_handled, 1);
    }

    #[tokio::test]
    async fn should_increase_the_tcp6_announces_counter_when_it_receives_a_tcp6_announce_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp6Announce, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp6_announces_handled, 1);
    }

    #[tokio::test]
    async fn should_increase_the_tcp6_connections_counter_when_it_receives_a_tcp6_announce_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp6Announce, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp6_connections_handled, 1);
    }

    #[tokio::test]
    async fn should_increase_the_tcp6_scrapes_counter_when_it_receives_a_tcp6_scrape_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp6Scrape, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp6_scrapes_handled, 1);
    }

    #[tokio::test]
    async fn should_increase_the_tcp6_connections_counter_when_it_receives_a_tcp6_scrape_event() {
        let stats_repository = Repository::new();

        handle_event(Event::Tcp6Scrape, &stats_repository).await;

        let stats = stats_repository.get_stats().await;

        assert_eq!(stats.tcp6_connections_handled, 1);
    }
}
